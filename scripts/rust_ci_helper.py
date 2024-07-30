#!/usr/bin/env python3

from collections import defaultdict

import argparse
import json
import os
import subprocess
import sys


def stderr(s):
    print(s, file=sys.stderr)


def stdout(s):
    print(s)


def run(command):
    assert isinstance(command, str)
    stderr(f"Running: {command}")
    exit_code = subprocess.check_call(command, shell=True, text=True)
    assert exit_code == 0


def run_with_stdout(command):
    assert isinstance(command, str)
    stderr(f"Running: {command}")
    cmd_output = subprocess.check_output(command, shell=True, text=True)
    return cmd_output


def find_cargo_deb_crates(*, workspace_crates):
    def predicate(package):
        m = package.get("metadata")
        return m is not None and "deb" in m

    return [p for p in workspace_crates if predicate(p)]


def find_unsupported_platform_crates(*, host_platform, workspace_crates):
    def predicate(package):
        tmp = package.get("metadata") or {}
        tmp = tmp.get("orb") or {}
        tmp = tmp.get("unsupported_targets") or {}
        if tmp == {}:
            return False
        return host_platform in tmp

    return set([c["name"] for c in workspace_crates if predicate(c)])


def workspace_crates():
    command = "cargo metadata --format-version=1 --no-deps --frozen --offline"
    cmd_output = run_with_stdout(command)
    metadata = json.loads(cmd_output)
    workspace_members = set(metadata["workspace_members"])

    return [p for p in metadata["packages"] if p["id"] in workspace_members]


def get_target_triple():
    cmd_output = run_with_stdout("rustc -vV").strip().split("\n")
    for s in cmd_output:
        if s.startswith("host:"):
            return s.split(" ")[1]
    raise Exception("no target triple detected")


def build_all_crates(*, cargo_profile, targets):
    targets_option = " ".join([f"--target {t}-unknown-linux-gnu" for t in targets])
    run(
        f"cargo zigbuild --all "
        f"--locked "  # ensures that the lockfile is up to date.
        f"--profile {cargo_profile} "
        f"{targets_option} "
        f"--no-default-features"
    )


def run_cargo_deb(*, out_dir, cargo_profile, targets, crate):
    crate_name = crate["name"]
    out = os.path.join(out_dir, crate_name)
    os.makedirs(out, exist_ok=True)
    stderr(f"Creating .deb packages for {crate_name} and copying to {out}:")
    for t in targets:
        run(
            f"cargo deb --no-build --no-strip "
            f"--profile {cargo_profile} "
            f"-p {crate_name} "
            f"--target {t}-unknown-linux-gnu "
            f"-o {out}/{crate_name}_{t}.deb"
        )


def get_binaries(*, workspace_crates):
    """returns map of crate name to set of binaries for that crate"""
    binaries = defaultdict(lambda: [])
    for c in workspace_crates:
        for t in c["targets"]:
            if t["kind"] != ["bin"]:
                continue
            binaries[c["name"]].append(t["name"])
    return {k: set(v) for k, v in binaries.items()}


def copy_cargo_binaries(*, out_dir, cargo_profile, targets, workspace_crates):
    wksp_binaries = get_binaries(workspace_crates=workspace_crates)
    for crate_name, binaries in wksp_binaries.items():
        out = os.path.join(out_dir, crate_name)
        os.makedirs(out, exist_ok=True)
        stderr(f"Copying binaries for {crate_name} to {out}:")
        for t in targets:
            target_dir = f"target/{t}-unknown-linux-gnu/{cargo_profile}"
            for b in binaries:
                run(
                    f"cp -L "
                    f"target/{t}-unknown-linux-gnu/{cargo_profile}/{b} "
                    f"{out}/{b}_{t}"
                )


def main():
    parser = argparse.ArgumentParser(description="Scripts for Rust in CI")
    subparsers = parser.add_subparsers()

    build = subparsers.add_parser(
        "build_linux_artifacts",
        description="Builds rust binaries and .deb packages for x86 and aarch64 linux",
    )
    build.add_argument(
        "--out_dir", required=True, help="Output directory for artifacts"
    )
    build.add_argument(
        "--cargo_profile",
        required=True,
        help="Cargo profile to use for compiling the crates",
    )
    build.set_defaults(entry_point=subcmd_build_linux_artifacts)

    excludes = subparsers.add_parser(
        "excludes", description="Detects crates that should be excluded when building"
    )
    excludes.set_defaults(entry_point=subcmd_excludes)

    args = parser.parse_args()
    args.entry_point(args)


def subcmd_build_linux_artifacts(args):
    """entry point for `build_linux_artifacts` subcommand"""
    targets = ["aarch64", "x86_64"]
    stderr("building all crates")
    build_all_crates(cargo_profile=args.cargo_profile, targets=targets)

    wksp_crates = workspace_crates()
    deb_crates = find_cargo_deb_crates(workspace_crates=wksp_crates)
    stderr(f"Running cargo deb for: {[c['name'] for c in deb_crates]}")
    for crate in deb_crates:
        run_cargo_deb(
            out_dir=args.out_dir,
            cargo_profile=args.cargo_profile,
            targets=targets,
            crate=crate,
        )
    copy_cargo_binaries(
        workspace_crates=wksp_crates,
        targets=targets,
        out_dir=args.out_dir,
        cargo_profile=args.cargo_profile,
    )


def subcmd_excludes(args):
    """entry point for `excludes` subcommand"""
    wksp_crates = workspace_crates()
    host = get_target_triple()
    excludes = find_unsupported_platform_crates(
        host_platform=host, workspace_crates=wksp_crates
    )
    stdout(" ".join(sorted([c for c in excludes])))


if __name__ == "__main__":
    main()
