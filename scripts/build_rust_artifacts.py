#!/usr/bin/env python3

import argparse
import subprocess
import sys
import os
import shlex


def run(command):
    assert isinstance(command, str)
    print(f"Running: {command}")
    exit_code = subprocess.check_call(command, shell=True, text=True)
    assert exit_code == 0


def run_with_stdout(command):
    assert isinstance(command, str)
    print(f"Running: {command}")
    cmd_output = subprocess.check_output(command, shell=True, text=True)
    return cmd_output


def find_cargo_deb_crates():
    jq_query = (
        ".workspace_members[] as $wm "
        "| .packages[ ] "
        "| .id as $id "
        "| select( $wm | contains($id)) "
        '| select( .metadata | has("deb")) '
        "| .name"
    )
    command = f"cargo metadata --format-version=1 | jq '{jq_query}'"
    cmd_output = subprocess.check_output(command, shell=True, text=True)
    crates = [c.strip('"') for c in cmd_output.strip().split("\n")]
    return crates


def build_all_crates(*, cargo_profile, targets):
    targets_option = " ".join([f"--target {t}-unknown-linux-gnu" for t in targets])
    run(
        f"cargo zigbuild --all "
        f"--profile {cargo_profile} "
        f"{targets_option} "
        f"--no-default-features"
    )


def run_cargo_deb(*, out_dir, cargo_profile, targets, crates):
    for c in crates:
        os.makedirs(os.path.join(out_dir, c), exist_ok=True)
        print(f"Creating .deb package for {c}:")
        for t in targets:
            run(
                f"cargo deb --no-build --no-strip "
                f"--profile {cargo_profile} "
                f"-p {c} "
                f"--target {t}-unknown-linux-gnu "
                f"-o {out_dir}/{c}/{c}_{t}.deb"
            )
            run(
                f"cp -L "
                f"target/{t}-unknown-linux-gnu/{cargo_profile}/{c} "
                f"{out_dir}/{c}/{c}_{t}"
            )


def main():
    parser = argparse.ArgumentParser(description="Builds rust artifacts for CI")
    parser.add_argument(
        "--out_dir", required=True, help="Output directory for artifacts"
    )
    parser.add_argument(
        "--cargo_profile",
        required=True,
        help="Cargo profile to use for compiling the crates",
    )
    args = parser.parse_args()

    targets = ["aarch64", "x86_64"]
    print("building all crates")
    build_all_crates(cargo_profile=args.cargo_profile, targets=targets)
    deb_crates = find_cargo_deb_crates()
    print(f"Running cargo deb for: {deb_crates}")
    run_cargo_deb(
        out_dir=args.out_dir,
        cargo_profile=args.cargo_profile,
        targets=targets,
        crates=deb_crates,
    )


if __name__ == "__main__":
    main()
