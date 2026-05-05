#!/usr/bin/env python3
import argparse
import json
import logging
import os
import subprocess
import sys
import tempfile
import shutil
import hashlib
from pathlib import Path
from typing import List, Tuple
import urllib.request
import urllib.error

REQUIRED_TOOLS = [
    "ssh-keygen",
    "mke2fs",
    "tune2fs",
    "mount",
    "umount",
    "install",
    "setfacl",
    "sync",
    "cloudflared",
]


def check_cli_dependencies(commands: List[str]):
    """Ensure all required CLI tools are available in PATH."""
    missing = []
    for cmd in commands:
        if shutil.which(cmd) is None:
            missing.append(cmd)

    if missing:
        print(
            f"Error: Missing required CLI dependencies: {', '.join(missing)}",
            file=sys.stderr,
        )
        sys.exit(1)


class ColorFormatter(logging.Formatter):
    """Manual logging setup to avoid extra dependencies"""

    def format(self, record) -> str:

        level_colors = {
            logging.DEBUG: "\033[37m",  # light gray
            logging.INFO: "\033[36m",  # cyan
            logging.WARNING: "\033[33m",  # yellow
            logging.ERROR: "\033[31m",  # red
            logging.CRITICAL: "\033[41m",  # red background
        }

        color = level_colors.get(record.levelno)
        original = super().format(record)
        return f"{color}{original}"


def generate_logger(level=logging.INFO) -> logging.Logger:
    logger = logging.getLogger()
    logger.setLevel(level)
    handler = logging.StreamHandler(sys.stdout)
    handler.setLevel(level)

    fmt = "[%(asctime)s]%(levelname)s: %(message)s"

    formatter = ColorFormatter(fmt, datefmt="%H:%M:%S")
    handler.setFormatter(formatter)

    # avoid duplicate handlers if called multiple times
    if not logger.hasHandlers():
        logger.addHandler(handler)
    else:
        logger.handlers = [handler]
    return logger


class OrbRegistration:
    def __init__(self, args):
        self.args = args
        self.logger = generate_logger()
        self.script_dir = Path(__file__).parent
        self.build_dir = self.script_dir / "build"
        self.artifacts_dir = self.script_dir / "artifacts"
        self.core_app_url = "https://api.operator.worldcoin.org/v1/graphql"
        self.persistent_size = 1024 * 1024
        self.persistent_journaled_size = 10 * 1024 * 1024

        if args.backend == "stage":
            self.domain = "https://management.internal.stage.orb.worldcoin.dev"
            if args.platform == "pearl":
                self.channel = "internal-testing"
            elif args.platform == "diamond":
                self.channel = "dev_diamond_channel"
        elif args.backend == "prod":
            self.domain = "https://management.internal.orb.worldcoin.dev"
            if args.platform == "diamond" and args.channel == "general":
                self.channel = "diamond-tier-ga"
            else:
                self.channel = args.channel
        else:
            raise ValueError(f"Invalid backend: {args.backend}")

    def check_orb_id_format(self, orb_id: str) -> str:

        if not orb_id.islower():
            self.logger.warning(
                f"Orb ID {orb_id} contains upper-cased chars and they will be lowered"
            )
            orb_id = orb_id.lower()

        if len(orb_id) < 8:
            self.logger.warning(
                f"Orb ID '{orb_id}' is less than 8 characters, padding with zeros"
            )
            orb_id = orb_id.zfill(8)
        elif len(orb_id) > 8:
            raise ValueError(f"Orb ID '{orb_id}' exceeds 8 characters")

        return orb_id

    def get_cloudflared_token(self) -> str:
        """Get Cloudflare access token for the domain."""
        self.logger.info(f"Logging in to Cloudflare Access for domain: {self.domain}")
        subprocess.run(
            ["cloudflared", "access", "login", "--quiet", self.domain], check=True
        )

        self.logger.info("Fetching Cloudflare access token")
        result = subprocess.run(
            ["cloudflared", "access", "token", f"--app={self.domain}"],
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout.strip()

    def detect_platform(self, hardware_version: str) -> str:
        """Detect platform from hardware version."""
        if hardware_version.startswith("PEARL_"):
            return "pearl"
        elif hardware_version.startswith("DIAMOND_"):
            return "diamond"
        else:
            raise ValueError(f"Unknown hardware version format: {hardware_version}")

    def generate_orb_id(self) -> str:
        """Generate new orb ID from SSH keypair (Pearl only)."""
        self.logger.info("Generating new SSH keypair to derive Orb ID...")

        subprocess.run(
            [
                "ssh-keygen",
                "-N",
                "",
                "-o",
                "-a",
                "100",
                "-t",
                "ed25519",
                "-q",
                "-f",
                str(self.build_dir / "uid"),
            ],
            check=True,
        )

        # Derive orb_id from public key
        with open(self.build_dir / "uid.pub", "r") as f:
            public_key = f.read().strip().split()[1]

        orb_id = hashlib.sha256(public_key.encode()).hexdigest()[:8]
        return orb_id

    def create_persistent_images(self, mount_point: Path):
        """Create base persistent images with required JSON files."""
        self.logger.info("Creating base persistent and persistent-journaled images...")

        persistent_img = self.build_dir / "persistent.img"
        persistent_journaled_img = self.build_dir / "persistent-journaled.img"

        # Create empty images
        self.logger.info(
            f"Creating empty images of size {self.persistent_size} and {self.persistent_journaled_size}"
        )

        with open(persistent_img, "wb") as f:
            f.write(b"\x00" * self.persistent_size)

        with open(persistent_journaled_img, "wb") as f:
            f.write(b"\x00" * self.persistent_journaled_size)

        # Format with ext4
        self.logger.info("Formatting persistent-journaled.img with ext4 (with journal)")
        subprocess.run(
            [
                "mke2fs",
                "-q",
                "-t",
                "ext4",
                "-E",
                "root_owner=0:1000",
                str(persistent_journaled_img),
            ],
            check=True,
        )

        self.logger.info("Formatting persistent.img with ext4 (no journal)")
        subprocess.run(
            [
                "mke2fs",
                "-q",
                "-t",
                "ext4",
                "-O",
                "^has_journal",
                "-E",
                "root_owner=0:1000",
                str(persistent_img),
            ],
            check=True,
        )

        # Set ACL support
        self.logger.info("Setting ACL support on both images")
        subprocess.run(
            ["tune2fs", "-o", "acl", str(persistent_journaled_img)],
            capture_output=True,
            check=True,
        )
        subprocess.run(
            ["tune2fs", "-o", "acl", str(persistent_img)],
            capture_output=True,
            check=True,
        )

        # Install baseline JSON files
        for img_name, img_path in [
            ("persistent.img", persistent_img),
            ("persistent-journaled.img", persistent_journaled_img),
        ]:
            self.logger.info(f"Mounting {img_name} and installing baseline JSON files")
            subprocess.run(
                ["mount", "-o", "loop", str(img_path), str(mount_point)], check=True
            )

            try:
                subprocess.run(
                    [
                        "install",
                        "-o",
                        "0",
                        "-g",
                        "1000",
                        "-m",
                        "664",
                        str(self.build_dir / "components.json"),
                        str(mount_point / "components.json"),
                    ],
                    check=True,
                )
                subprocess.run(
                    [
                        "install",
                        "-o",
                        "1000",
                        "-g",
                        "1000",
                        "-m",
                        "664",
                        str(self.build_dir / "calibration.json"),
                        str(mount_point / "calibration.json"),
                    ],
                    check=True,
                )
                subprocess.run(
                    [
                        "install",
                        "-o",
                        "1000",
                        "-g",
                        "1000",
                        "-m",
                        "664",
                        str(self.build_dir / "versions.json"),
                        str(mount_point / "versions.json"),
                    ],
                    check=True,
                )
                subprocess.run(
                    ["setfacl", "-d", "-m", "u::rwx,g::rwx,o::rx", str(mount_point)],
                    check=True,
                )
                subprocess.run(["sync"], check=True)
            finally:
                subprocess.run(["umount", str(mount_point)], check=True)

    def fetch_existing_orb_name(self, orb_id: str, cf_token: str) -> str:
        """Fetch existing orb name if already registered."""
        url = f"{self.domain}/api/v1/orbs/{orb_id}/details"
        req = urllib.request.Request(url, method="GET")

        req.add_header("Authorization", f"Bearer {self.args.mongo_token}")
        req.add_header("cf-access-token", cf_token)
        req.add_header("User-Agent", "curl/8.1.2")

        try:
            with urllib.request.urlopen(req) as response:
                result = json.loads(response.read().decode())
                name = result.get("Name")
                if not name:
                    raise ValueError(f"No name found for orb {orb_id} in response.")
                return name
        except urllib.error.HTTPError as e:
            self.logger.error(
                f"Failed to fetch existing orb details for {orb_id}: HTTP {e.code} {e.reason}"
            )
            raise

    def register_orb_mongo(self, orb_id: str, cf_token: str, platform: str) -> str:
        """Register orb in MongoDB and return orb_name. If already exists, fetch name."""
        self.logger.info(f"Creating Orb record in Management API for Orb ID: {orb_id}")

        data = {
            "BuildVersion": self.args.hardware_version,
            "ManufacturerName": self.args.manufacturer,
            "Platform": platform,
        }

        req = urllib.request.Request(
            f"{self.domain}/api/v1/orbs/{orb_id}",
            data=json.dumps(data).encode(),
            method="POST",
        )

        req.add_header("Content-Type", "application/json")
        req.add_header("Authorization", f"Bearer {self.args.mongo_token}")
        req.add_header("cf-access-token", cf_token)
        req.add_header("User-Agent", "curl/8.1.2")

        try:
            with urllib.request.urlopen(req) as response:
                result = json.loads(response.read().decode())
                return result["name"]
        except urllib.error.HTTPError as e:
            if e.code == 409:
                self.logger.warning(
                    f"Orb ID {orb_id} already registered. Fetching existing details..."
                )
                return self.fetch_existing_orb_name(orb_id, cf_token)
            else:
                error_msg = f"Failed to register orb {orb_id} in MongoDB: HTTP {e.code} {e.reason}"
                try:
                    error_response = e.read().decode()
                    if error_response:
                        try:
                            error_json = json.loads(error_response)
                            error_msg += f" - {error_json}"
                        except json.JSONDecodeError:
                            error_msg += f" - {error_response}"
                except:
                    pass
                self.logger.error(error_msg)
                raise

    def register_orb_core_app(self, orb_id: str, orb_name: str):
        """Register orb in Core-App."""

        is_dev = self.args.release == "dev"

        query = """
                mutation InsertOrb(
                    $deviceId: String, 
                    $name: String!, 
                    $deviceType: orbDeviceTypeEnum_enum!, 
                    $isDevelopment: Boolean!
                ) {
                    insert_orb(
                        objects: [{
                            name: $name, 
                            deviceId: $deviceId, 
                            status: FLASHED, 
                            deviceType: $deviceType, 
                            isDevelopment: $isDevelopment
                        }], 
                        on_conflict: {constraint: orb_pkey}
                    ) {
                        affected_rows
                    }
                }
            """

        data = {
            "query": query,
            "variables": {
                "deviceId": orb_id,
                "name": orb_name,
                "deviceType": self.args.hardware_version,  # e.g., "DIAMOND_EVT"
                "isDevelopment": is_dev,  # True/False
            },
        }

        headers = {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {self.args.core_token}",
        }

        req = urllib.request.Request(
            self.core_app_url,
            data=json.dumps(data).encode(),
            headers=headers,
            method="POST",
        )

        try:
            with urllib.request.urlopen(req) as response:
                result = json.loads(response.read().decode())
                if (
                    result.get("data", {}).get("insert_orb", {}).get("affected_rows")
                    != 1
                ):
                    print("GraphQL Response:", json.dumps(result, indent=2))
                    raise ValueError("Failed to register Orb in Core-App")
                self.logger.info(f"Orb {orb_id} registered successfully in Core-App")
        except urllib.error.HTTPError as e:
            error_msg = (
                f"Failed to register orb {orb_id} in Core-App: HTTP {e.code} {e.reason}"
            )
            try:
                error_response = e.read().decode()
                if error_response:
                    try:
                        error_json = json.loads(error_response)
                        error_msg += f" - {error_json}"
                    except json.JSONDecodeError:
                        error_msg += f" - {error_response}"
            except:
                pass
            self.logger.error(error_msg)
            raise

    def set_orb_channel(self, orb_id: str, cf_token: str):
        """Set orb channel in MongoDB."""
        self.logger.info(f"Setting Orb channel to '{self.channel}'")

        data = {"channel": self.channel}

        req = urllib.request.Request(
            f"{self.domain}/api/v1/orbs/{orb_id}/channel",
            data=json.dumps(data).encode(),
            method="POST",
        )

        # Add headers manually to preserve exact case
        req.add_header("Content-Type", "application/json")
        req.add_header("Authorization", f"Bearer {self.args.mongo_token}")
        req.add_header("cf-access-token", cf_token)
        req.add_header("User-Agent", "curl/8.1.2")

        try:
            with urllib.request.urlopen(req) as _:
                # Success if no exception
                pass
        except urllib.error.HTTPError as e:
            error_msg = (
                f"Failed to set channel for orb {orb_id}: HTTP {e.code} {e.reason}"
            )
            try:
                error_response = e.read().decode()
                if error_response:
                    try:
                        error_json = json.loads(error_response)
                        error_msg += f" - {error_json}"
                    except json.JSONDecodeError:
                        error_msg += f" - {error_response}"
            except:
                pass
            self.logger.error(error_msg)
            raise

    def get_orb_token(self, orb_id: str, cf_token: str) -> str:
        """Get orb token from MongoDB."""
        self.logger.info("Fetching Orb token from Management API")

        req = urllib.request.Request(
            f"{self.domain}/api/v1/tokens?orbId={orb_id}",
            data=b"{}",
            method="POST",
        )

        req.add_header("Content-Type", "application/json")
        req.add_header("Authorization", f"Bearer {self.args.mongo_token}")
        req.add_header("cf-access-token", cf_token)
        req.add_header("User-Agent", "curl/8.1.2")

        try:
            with urllib.request.urlopen(req) as response:
                result = json.loads(response.read().decode())
                return result["token"]
        except urllib.error.HTTPError as e:
            error_msg = (
                f"Failed to get token for orb {orb_id}: HTTP {e.code} {e.reason}"
            )
            try:
                error_response = e.read().decode()
                if error_response:
                    try:
                        error_json = json.loads(error_response)
                        error_msg += f" - {error_json}"
                    except json.JSONDecodeError:
                        error_msg += f" - {error_response}"
            except:
                pass
            self.logger.error(error_msg)
            raise

    def save_orb_artifacts(
        self, orb_id: str, orb_name: str, token: str, mount_point: Path
    ):
        """Save orb artifacts (Pearl only)."""
        jet_artifacts_dir = self.artifacts_dir / orb_id
        jet_artifacts_dir.mkdir(parents=True, exist_ok=True)

        # Move SSH keys
        shutil.move(str(self.build_dir / "uid"), str(jet_artifacts_dir / "uid"))
        shutil.move(str(self.build_dir / "uid.pub"), str(jet_artifacts_dir / "uid.pub"))

        # Save orb name and token
        with open(jet_artifacts_dir / "orb-name", "w") as f:
            f.write(orb_name)
        with open(jet_artifacts_dir / "token", "w") as f:
            f.write(token)

        # Copy persistent images
        self.logger.info(
            f"Copying base persistent images into artifacts directory for {orb_id}"
        )
        shutil.copy(
            str(self.build_dir / "persistent.img"),
            str(jet_artifacts_dir / "persistent.img"),
        )
        shutil.copy(
            str(self.build_dir / "persistent-journaled.img"),
            str(jet_artifacts_dir / "persistent-journaled.img"),
        )

        # Mount and install orb-name and token in both images
        for img_name in ["persistent.img", "persistent-journaled.img"]:
            self.logger.info(f"Mounting {img_name} for Orb ID: {orb_id}")
            subprocess.run(
                ["mount", str(jet_artifacts_dir / img_name), str(mount_point)],
                check=True,
            )

            try:
                subprocess.run(
                    [
                        "install",
                        "-o",
                        "0",
                        "-g",
                        "0",
                        "-m",
                        "644",
                        str(jet_artifacts_dir / "orb-name"),
                        str(mount_point / "orb-name"),
                    ],
                    check=True,
                )
                subprocess.run(
                    [
                        "install",
                        "-o",
                        "0",
                        "-g",
                        "0",
                        "-m",
                        "644",
                        str(jet_artifacts_dir / "token"),
                        str(mount_point / "token"),
                    ],
                    check=True,
                )
                subprocess.run(["sync"], check=True)
            finally:
                subprocess.run(["umount", str(mount_point)], check=True)

    def process_pearl_orb(self, cf_token: str, mount_point: Path) -> str:
        """Process a single Pearl orb (generate ID, register, create artifacts)."""
        orb_id = self.generate_orb_id()
        platform = self.detect_platform(self.args.hardware_version)

        orb_name = self.register_orb_mongo(orb_id, cf_token, platform)
        self.set_orb_channel(orb_id, cf_token)
        token = self.get_orb_token(orb_id, cf_token)

        self.save_orb_artifacts(orb_id, orb_name, token, mount_point)
        self.register_orb_core_app(orb_id, orb_name)

        return orb_id

    def process_diamond_orb_ids(self, orb_ids: List[str], cf_token: str):
        """Process Diamond orb IDs (register in MongoDB, then Core-App)."""
        platform = self.detect_platform(self.args.hardware_version)

        for orb_id in orb_ids:
            self.logger.info(f"Processing Diamond Orb ID: {orb_id}")
            orb_id = self.check_orb_id_format(orb_id)
            orb_name = self.register_orb_mongo(orb_id, cf_token, platform)
            self.register_orb_core_app(orb_id, orb_name)
            self.logger.info(f"Successfully processed Diamond Orb: {orb_id}")

    def process_diamond_orb_pairs(self, orb_pairs: List[Tuple[str, str]]):
        """Process Diamond orb ID+name pairs (register directly in Core-App)."""
        for orb_id, orb_name in orb_pairs:
            self.logger.info(f"Processing Diamond Orb pair: {orb_id} -> {orb_name}")
            orb_id = self.check_orb_id_format(orb_id)
            self.register_orb_core_app(orb_id, orb_name)
            self.logger.info(
                f"Successfully processed Diamond Orb pair: {orb_id} -> {orb_name}"
            )

    def read_input_file(self, file_path: str) -> List[str]:
        """Read orb IDs from input file."""
        with open(file_path, "r") as f:
            return [line.strip() for line in f if line.strip()]

    def read_input_pairs_file(self, file_path: str) -> List[Tuple[str, str]]:
        """Read orb ID+name pairs from input file."""
        pairs = []
        with open(file_path, "r") as f:
            for line in f:
                line = line.strip()
                if line:
                    parts = line.split()
                    if len(parts) != 2:
                        raise ValueError(f"Invalid line format: {line}")
                    pairs.append((parts[0], parts[1]))
        return pairs

    def run(self):
        """Main execution logic."""
        cf_token = self.get_cloudflared_token()

        if self.args.platform == "pearl":
            # Pearl: Generate artifacts and register
            self.build_dir.mkdir(exist_ok=True)
            self.artifacts_dir.mkdir(exist_ok=True)

            with tempfile.TemporaryDirectory() as temp_dir:
                mount_point = Path(temp_dir) / "loop"
                mount_point.mkdir()

                self.create_persistent_images(mount_point)

                for i in range(self.args.count):
                    self.logger.info(
                        f"Generating Pearl Orb ID #{i+1} of {self.args.count}..."
                    )
                    orb_id = self.process_pearl_orb(cf_token, mount_point)
                    self.logger.info(f"Successfully processed Pearl Orb: {orb_id}")
                    print("", file=sys.stderr)

                self.logger.info(
                    f"All {self.args.count} Pearl Orb IDs generated and registered successfully."
                )

        elif self.args.platform == "diamond":
            if self.args.input_file:
                # Diamond: Read from file
                if self.args.input_format == "ids":
                    orb_ids = self.read_input_file(self.args.input_file)
                    self.process_diamond_orb_ids(orb_ids, cf_token)
                elif self.args.input_format == "pairs":
                    orb_pairs = self.read_input_pairs_file(self.args.input_file)
                    self.process_diamond_orb_pairs(orb_pairs)
            elif self.args.orb_ids:
                # Diamond: Direct arguments
                self.process_diamond_orb_ids(self.args.orb_ids, cf_token)
            else:
                raise ValueError(
                    "Diamond platform requires either --input-file or direct orb IDs"
                )


def main():
    parser = argparse.ArgumentParser(
        description="Generate and register Orb IDs for both Pearl and Diamond platforms"
    )

    parser.add_argument(
        "--platform",
        choices=["pearl", "diamond"],
        required=True,
        help="Platform type (pearl or diamond)",
    )
    parser.add_argument(
        "--backend",
        choices=["stage", "prod"],
        required=True,
        help="Backend environment",
    )
    parser.add_argument(
        "--release", choices=["dev", "prod"], required=True, help="Release type"
    )
    parser.add_argument(
        "--hardware-version",
        required=True,
        help="Hardware version (e.g., PEARL_EVT1, DIAMOND_EVT2)",
    )

    parser.add_argument(
        "--manufacturer",
        default="TFH_Jabil",
        help="Manufacturer name for orb registration (default: TFH_Jabil)",
    )

    parser.add_argument(
        "--mongo-token", help="MongoDB bearer token (overrides environment)"
    )
    parser.add_argument(
        "--core-token", help="Core-App bearer token (overrides environment)"
    )
    parser.add_argument(
        "--channel",
        default="general",
        help="Channel to set for orb registration (default: general)",
    )

    # Pearl-specific arguments
    parser.add_argument(
        "--count",
        type=int,
        default=1,
        help="Number of Pearl orbs to generate (Pearl only)",
    )

    # Diamond-specific arguments
    parser.add_argument(
        "--input-file",
        help="Input file containing orb IDs or orb ID+name pairs (Diamond only)",
    )
    parser.add_argument(
        "--input-format",
        choices=["ids", "pairs"],
        default="ids",
        help="Format of input file: 'ids' for orb IDs only, 'pairs' for orb ID+name pairs",
    )
    parser.add_argument(
        "orb_ids",
        nargs="*",
        help="Orb IDs to register (Diamond only, alternative to --input-file)",
    )

    check_cli_dependencies(REQUIRED_TOOLS)
    args = parser.parse_args()

    # Set default tokens from environment
    if not args.mongo_token:
        args.mongo_token = os.environ.get("FM_CLI_ORB_MANAGER_INTERNAL_TOKEN")
    if not args.core_token:
        args.core_token = os.environ.get("HARDWARE_TOKEN_PRODUCTION")

    # Validate required tokens
    if not args.mongo_token or not args.core_token:
        print("Error: Missing required tokens", file=sys.stderr)
        print(
            f"MongoDB token: {'SET' if args.mongo_token else 'NOT SET'}",
            file=sys.stderr,
        )
        print(
            f"Core-App token: {'SET' if args.core_token else 'NOT SET'}",
            file=sys.stderr,
        )
        sys.exit(1)

    # Platform-specific validation
    if args.platform == "pearl":
        if args.input_file or args.orb_ids:
            print(
                "Error: Pearl platform doesn't support input files or direct orb IDs",
                file=sys.stderr,
            )
            sys.exit(1)
    elif args.platform == "diamond":
        if not args.input_file and not args.orb_ids:
            print(
                "Error: Diamond platform requires either --input-file or direct orb IDs",
                file=sys.stderr,
            )
            sys.exit(1)
        if args.input_file and args.orb_ids:
            print(
                "Error: Cannot use both --input-file and direct orb IDs",
                file=sys.stderr,
            )
            sys.exit(1)

    try:
        orb_registration = OrbRegistration(args)
        orb_registration.run()
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
