#!/usr/bin/env python3

import argparse
import subprocess
import sys
import os
import shlex


def cmd(command):
    assert isinstance(command, str)
    # This allows us to avoid manually splitting strings into arguments
    command = shlex.split(command)
    print(f"Running: {' '.join(command)}")
    subprocess.check_call(command)


def main():
    parser = argparse.ArgumentParser(description="Builds rust artifacts for CI")
    parser.add_argument("out_dir", help="Output directory for artifacts")
    parser.add_argument(
        "cargo_profile", help="Cargo profile to use for compiling the crates"
    )
    parser.add_argument("crates", nargs="+", help="List of crate names to be processed")

    args = parser.parse_args()

    targets = ["aarch64", "x86_64"]

    targets_option = " ".join([f"--target {t}-unknown-linux-gnu" for t in targets])
    print(f"TARGETS={targets_option}")

    cmd(
        f"cargo zigbuild --all "
        f"--profile {args.cargo_profile} "
        f"{targets_option} "
        f"--no-default-features"
    )

    for b in args.crates:
        os.makedirs(os.path.join(args.out_dir, b), exist_ok=True)
        print(f"Creating .deb package for {b}:")
        for t in targets:
            cmd(
                f"cargo deb --no-build --no-strip "
                f"--profile {args.cargo_profile} "
                f"-p {b} "
                f"--target {t}-unknown-linux-gnu "
                f"-o {args.out_dir}/{b}/{b}_{t}.deb"
            )
            cmd(
                f"cp -L "
                f"target/{t}-unknown-linux-gnu/{args.cargo_profile}/{b} "
                f"{args.out_dir}/{b}/{b}_{t}"
            )


if __name__ == "__main__":
    main()
