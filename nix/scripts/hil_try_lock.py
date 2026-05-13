#!/usr/bin/env python3
"""try_lock: lock this HIL on the orb-hil-orchestrator if it's idle.

Reads `HIL_ORCHESTRATOR_URL` and `HIL_ORB_ID` from the environment (set by the
NixOS wrapper).  Optional positional args become a note shown on the dashboard
so other engineers know why this runner is locked.
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
    note = " ".join(sys.argv[1:]).strip() or None

    runners = requests.get(f"{orchestrator}/runners", timeout=TIMEOUT).json()
    me = next((r for r in runners if r.get("id") == orb_id), None)
    current_job = (me or {}).get("current_job")
    if current_job:
        print(
            f"try_lock: refusing — runner busy on job {current_job}",
            file=sys.stderr,
        )
        return 1
    if (me or {}).get("locked"):
        print("try_lock: refusing — runner is already locked", file=sys.stderr)
        return 1

    resp = requests.post(f"{orchestrator}/runners/{orb_id}/lock", timeout=TIMEOUT)
    if resp.status_code != 200:
        print(
            f"try_lock failed (HTTP {resp.status_code}): {_error_msg(resp)}",
            file=sys.stderr,
        )
        return 1
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
