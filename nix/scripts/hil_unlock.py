#!/usr/bin/env python3
"""unlock: queue an unlock on the orb-hil-orchestrator and clear the note.

Reads `HIL_ORCHESTRATOR_URL` and `HIL_ORB_ID` from the environment (set by the
NixOS wrapper).
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

    resp = requests.post(f"{orchestrator}/runners/{orb_id}/unlock", timeout=TIMEOUT)
    if resp.status_code != 200:
        print(
            f"unlock failed (HTTP {resp.status_code}): {_error_msg(resp)}",
            file=sys.stderr,
        )
        return 1
    print(f"unlock queued for {orb_id}")

    requests.delete(f"{orchestrator}/runners/{orb_id}/note", timeout=TIMEOUT)

    return 0


if __name__ == "__main__":
    sys.exit(main())
