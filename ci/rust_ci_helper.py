#!/usr/bin/env python3
# TODO: Rewrite this whole script in rust using cargo xtask. Its ridiculous how
# annoying it is to not have any type info.

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


def find_binary_crates(*, workspace_crates):
    def predicate(package):
        for t in package["targets"]:
            if t["kind"] == ["bin"]:
                return True
        return False

    return {n: p for n, p in workspace_crates.items() if predicate(p)}


def find_cargo_deb_crates(*, workspace_crates):
    def predicate(package):
        m = package.get("metadata")
        return m is not None and "deb" in m

    return {n: p for n, p in workspace_crates.items() if predicate(p)}


def find_flavored_crates(*, workspace_crates):
    def predicate(package):
        tmp = package.get("metadata") or {}
        tmp = tmp.get("orb") or {}
        flavors = tmp.get("flavors") or {}
        if flavors == {}:
            return False
        if not isinstance(flavors, list):
            raise ValueError("`flavors` must be a list")
        for f in flavors:
            if f.get("name") is None:
                raise ValueError(f"missing `name` field for flavor {f}")
            features = f.get("features")
            if features is None:
                raise ValueError(f"missing `features` field for flavor {f}")
            if not isinstance(features, list):
                raise ValueError(f"`features` must be a list")
        return True

    return {n: p for n, p in workspace_crates.items() if predicate(p)}


def find_unsupported_platform_crates(*, host_platform, workspace_crates):
    def predicate(package):
        tmp = package.get("metadata") or {}
        tmp = tmp.get("orb") or {}
        tmp = tmp.get("unsupported_targets") or {}
        if tmp == {}:
            return False
        return host_platform in tmp

    return {n: p for n, p in workspace_crates.items() if predicate(p)}


def workspace_crates():
    command = "cargo metadata --format-version=1 --no-deps --frozen --offline"
    cmd_output = run_with_stdout(command)
    metadata = json.loads(cmd_output)
    workspace_members = set(metadata["workspace_members"])

    tmp = [p for p in metadata["packages"] if p["id"] in workspace_members]
    result = {p["name"]: p for p in tmp}
    assert len(tmp) == len(result)  # sanity check
    return result


def get_target_triple():
    cmd_output = run_with_stdout("rustc -vV").strip().split("\n")
    for s in cmd_output:
        if s.startswith("host:"):
            return s.split(" ")[1]
    raise Exception("no target triple detected")


def build_crate_with_features(*, cargo_profile, targets, features):
    targets_option = " ".join([f"--target {t}-unknown-linux-gnu" for t in targets])
    feature_option = " ".join([f"--features {f}" for f in features])
    run(
        f"cargo zigbuild --all "
        f"--locked "  # ensures that the lockfile is up to date.
        f"--profile {cargo_profile} "
        f"{targets_option} "
        f"--no-default-features "
        f"{feature_option}"
    )


def build_all_crates(*, cargo_profile, targets):
    targets_option = " ".join([f"--target {t}-unknown-linux-gnu" for t in targets])
    run(
        f"cargo zigbuild --all "
        f"--locked "  # ensures that the lockfile is up to date.
        f"--profile {cargo_profile} "
        f"{targets_option} "
        f"--no-default-features"
    )


def run_cargo_deb(*, out_dir, cargo_profile, targets, crate, flavor=None):
    crate_name = crate["name"]
    out = os.path.join(out_dir, crate_name)
    os.makedirs(out, exist_ok=True)
    stderr(f"Creating .deb packages for {crate_name} and copying to {out}:")
    for t in targets:
        if flavor is None:
            output_deb_path = f"{out}/{crate_name}_{t}.deb"
        else:
            output_deb_path = f"{out}/{crate_name}_{flavor}_{t}.deb"
        run(
            f"cargo deb --no-build --no-strip "
            f"--profile {cargo_profile} "
            f"-p {crate_name} "
            f"--target {t}-unknown-linux-gnu "
            f"-o {output_deb_path}"
        )
        # Ensures that the .deb actually contains the binary
        run(
            f"dpkg --contents {output_deb_path} | grep -E 'usr(/local)?/bin/{crate_name}'"
        )


def get_binaries(*, crate):
    """returns set of binaries for that crate"""
    binaries = []
    for t in crate["targets"]:
        if t["kind"] != ["bin"]:
            continue
        binaries.append(t["name"])
    return set(binaries)


def get_crate_flavors(*, crate):
    """extracts a dictionary of flavor_name => list[feature] for a given
    crate's metadata"""
    flavors = crate.get("metadata") or {}
    flavors = flavors.get("orb") or {}
    flavors = flavors.get("flavors") or {}
    return {f["name"]: f["features"] for f in flavors}


def copy_cargo_binaries(*, out_dir, cargo_profile, targets, crate, flavor=None):
    binaries = get_binaries(crate=crate)
    if len(binaries) == 0:
        raise ValueError(f"crate {crate} has no binaries")

    flavors = get_crate_flavors(crate=crate)
    if flavor is not None and not flavor in flavors:
        raise ValueError(
            f"expected flavor {flavor} to be present, instead flavors were: {flavors}"
        )

    crate_name = crate["name"]
    out = os.path.join(out_dir, crate_name)
    os.makedirs(out, exist_ok=True)
    stderr(f"Copying binaries: name={crate_name}, flavor={flavor}, out={out}:")
    for t in targets:
        target_dir = f"target/{t}-unknown-linux-gnu/{cargo_profile}"
        for b in binaries:
            if flavor is None:
                out_path = f"{out}/{b}_{t}"
            else:
                out_path = f"{out}/{b}_{flavor}_{t}"
            run(f"cp target/{t}-unknown-linux-gnu/{cargo_profile}/{b} {out_path}")


def is_valid_flavor_name(name):
    """Validates that the flavor name conforms to some naming scheme"""
    return (not "." in name) and (not "_" in name) and (not " " in name)


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
    wksp_crates = workspace_crates()
    deb_crates = find_cargo_deb_crates(workspace_crates=wksp_crates)
    binary_crates = find_binary_crates(workspace_crates=wksp_crates)
    flavored_crates = find_flavored_crates(workspace_crates=wksp_crates)

    for name in deb_crates:
        # sanity check: all deb crates should also be binary crates
        assert name in binary_crates
    for name in flavored_crates:
        # sanity check: all flavored crates should also be binary crates
        assert name in binary_crates
        # sanity check: all flavor names must be valid
        assert is_valid_flavor_name(name)

    # First, we will build all crates and their debs without any flavoring
    stderr("Building all crates: flavor=default")
    build_all_crates(cargo_profile=args.cargo_profile, targets=targets)
    for crate_name, crate in binary_crates.items():
        copy_cargo_binaries(
            crate=crate,
            targets=targets,
            out_dir=args.out_dir,
            cargo_profile=args.cargo_profile,
            flavor=None,
        )
    for crate_name, crate in deb_crates.items():
        stderr(f"Running cargo deb: name={crate_name}, flavor=default")
        run_cargo_deb(
            out_dir=args.out_dir,
            cargo_profile=args.cargo_profile,
            targets=targets,
            crate=crate,
            flavor=None,
        )

    # Next, we handle flavors
    stderr("building flavored crates")
    for crate_name, crate in flavored_crates.items():
        flavors = get_crate_flavors(crate=crate)
        # ensure that
        for flavor_name, features in flavors.items():
            stderr(f"Building crate: name={crate_name}, flavor={flavor_name}")
            build_crate_with_features(
                cargo_profile=args.cargo_profile,
                targets=targets,
                features=features,
            )
            copy_cargo_binaries(
                crate=crate,
                targets=targets,
                out_dir=args.out_dir,
                cargo_profile=args.cargo_profile,
                flavor=flavor_name,
            )
            if crate_name not in deb_crates:
                continue
            stderr(f"Running cargo deb: name={crate_name}, flavor={flavor_name}")
            run_cargo_deb(
                out_dir=args.out_dir,
                cargo_profile=args.cargo_profile,
                targets=targets,
                crate=crate,
                flavor=flavor_name,
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
