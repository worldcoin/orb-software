#!/usr/bin/env python3


import argparse
import subprocess
import sys
import os
import shlex


def run_with_stdout(command):
    assert isinstance(command, str)
    print(f"Running: {command}", file=sys.stderr)
    cmd_output = subprocess.check_output(command, shell=True, text=True)
    return cmd_output


def get_target_triple():
    cmd_output = run_with_stdout("rustc -vV").strip().split("\n")
    for s in cmd_output:
        if s.startswith("host:"):
            return s.split(" ")[1]
    raise Exception("no target triple detected")


def main():
    target = get_target_triple()
    print(f"identified target triple as: {target}", file=sys.stderr)
    jq_query = (
        ".workspace_members[] as $wm"
        "| .packages[ ] "
        "| .id as $id "
        "| select( $wm | contains($id)) "
        "| .name as $n "
        "| select (.metadata.orb.unsupported_targets "
        f'| index("{target}") != null) | .name'
    )

    command = f"cargo metadata --format-version=1 | jq -r '{jq_query}'"
    cmd_output = run_with_stdout(command).strip()
    print(cmd_output)


if __name__ == "__main__":
    main()
