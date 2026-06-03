#!/usr/bin/env python3
"""try_lock: lock this HIL on the orb-hil-orchestrator.

Reads `HIL_ORCHESTRATOR_URL` and `HIL_ORB_ID` from the environment (set by the
NixOS wrapper).  Optional positional args become a note shown on the dashboard
so other engineers know why this runner is locked.

The server enforces the only real safety constraint: it rejects the request
"""

from __future__ import annotations

import os
import sys

import requests


TIMEOUT = 10


def _error_msg(resp: requests.Response) -> str:
    try:
        return resp.json().get("error") or resp.text
    except ValueError:
        return resp.text


def main() -> int:
    orchestrator = os.environ["HIL_ORCHESTRATOR_URL"]
    orb_id = os.environ["HIL_ORB_ID"]
    first = sys.argv[1] if len(sys.argv) > 1 else None
    if first in ("diamond", "pearl"):
        platform = first
        note = " ".join(sys.argv[2:]).strip() or None
    else:
        platform = None
        note = " ".join(sys.argv[1:]).strip() or None

    if platform:
        resp = requests.post(f"{orchestrator}/lock/{platform}", timeout=TIMEOUT)
    else:
        resp = requests.post(f"{orchestrator}/runners/{orb_id}/lock", timeout=TIMEOUT)

    if resp.status_code != 200:
        print(
            f"try_lock failed (HTTP {resp.status_code}): {_error_msg(resp)}",
            file=sys.stderr,
        )
        return 1
    if platform:
        locked_runner = resp.json().get("runner_id", "unknown")
        print(f"lock queued for {locked_runner}")
    else:
        print(f"lock queued for {orb_id}")

    if note:
        resp = requests.put(
            f"{orchestrator}/runners/{orb_id}/note",
            json={"note": note},
            timeout=TIMEOUT,
        )
        if resp.status_code == 204:
            print(f"note set: {note}")
        else:
            print(
                f"try_lock: lock queued but note PUT failed "
                f"(HTTP {resp.status_code})",
                file=sys.stderr,
            )

    return 0


if __name__ == "__main__":
    sys.exit(main())
