---
name: investigate-orb
description: Use when diagnosing what happened on a field Orb, especially when given an orb_id tag, a service tag, an incident time, or symptoms involving health, connectivity, MCUs, attestation, updates, remote jobs, backend reporting, or signups.
---

# Investigate Orb

## Overview

Investigate field Orbs from Datadog evidence. Build a UTC chronology, distinguish observations from inferences, and treat missing telemetry as an unknown rather than proof of health.

## Establish Scope

Extract these values from the request:

- `orb_id:<orb_id>`: required. Ask for it only when the context does not identify one Orb.
- `service:<service_name>`: optional. Use it as the starting service, not the investigation boundary.
- Incident window: use the supplied bounds. Otherwise start at `1h` and widen to `6h` or `24h` only when needed.
- Symptom: select related services from [references/services.md](references/services.md).

Keep this skill's work read-only and limited to Datadog. Do not SSH into the Orb, run remote commands, restart services, create probes or downtimes, deploy software, or investigate the whole fleet unless the user explicitly expands the scope.

## Use Pup

1. If `dd-pup` is available in the current skill catalog, read and follow it.
2. Check for the CLI with `command -v pup`.
3. When both are available, use the skill's authentication guidance and the `pup` CLI.
4. When only the CLI is available, inspect `pup logs search --help` before querying.
5. When the CLI is absent, report the missing dependency. Do not install it without authorization.

Check authentication with `pup auth status`. On authentication failure, follow `dd-pup` when available; otherwise report the failure and the required login or permission. Never expose credentials or raw configuration.

Pass `--read-only` to every Pup data query. Prefer JSON output so timestamps, tags, and attributes remain inspectable.

## Investigation Loop

1. Query the Orb without a service filter. Confirm that `orb_id` is the correct facet and discover the actual `service`, `host`, `source`, version, and status fields.
2. Query the requested or symptom-selected service over the same window.
3. Order results ascending and build a timeline of state changes, starts, exits, restarts, warnings, errors, and recovery.
4. Expand only to dependencies named in the service reference or revealed by the evidence.
5. If a query reaches its limit, split the time window into smaller slices. Do not assume the truncated result is complete.
6. Widen the time window only when the current evidence does not show the onset or recovery.
7. Stop when evidence answers the question or when a specific missing signal blocks further Datadog investigation.

Start with:

```bash
pup logs search \
  --query "orb_id:<orb_id>" \
  --from 1h \
  --sort asc \
  --limit 100 \
  --output json \
  --read-only
```

Then scope by service:

```bash
pup logs search \
  --query "orb_id:<orb_id> service:<service_name>" \
  --from 1h \
  --sort asc \
  --limit 100 \
  --output json \
  --read-only
```

Use `--from` and `--to` with explicit RFC3339 timestamps when the incident window is known. Preserve the user's timezone in the report, but correlate evidence in UTC.

## Interpret Evidence

- Treat repeated starts after the configured `RestartSec` as evidence of a restart loop only when logs show distinct process lifecycles.
- Treat a graceful stop, crash, reboot, dependency failure, and telemetry loss as separate hypotheses.
- Correlate a service failure with its dependencies before assigning root cause.
- Treat absent logs as inconclusive. Check whether other services on the same Orb continued reporting.
- If every service disappears together, consider connectivity, power, reboot, `datadog-agent`, monitoring authentication, or ingestion failure.
- Do not infer a deployed service tag from a unit filename. Confirm tags from the broad Orb query.
- Use metrics, traces, or events only when the repository or returned logs identify a concrete signal relevant to the hypothesis. Inspect the Pup subcommand help first and keep the query read-only.

## Report

Lead with the narrowest supported conclusion. Include:

1. Orb ID, UTC window, original timezone, and services queried.
2. Finding and confidence: confirmed, likely, or inconclusive.
3. Short chronological evidence table with timestamps and sources.
4. Supporting and contradicting evidence for the leading hypothesis.
5. Telemetry gaps, query limits, tag uncertainty, and other limitations.
6. The smallest useful next query or separately authorized action.

Quote only short log fragments needed to identify an event. Never claim root cause from temporal proximity alone.

## Common Mistakes

- Starting with `service:` and missing an Orb-wide reboot or telemetry outage.
- Querying every related service before establishing a timeline.
- Omitting `--read-only`.
- Treating `check-my-orb` as a continuously running daemon; it is an on-demand composite diagnostic.
- Treating silence as health or as proof that a process stopped.
- Expanding into SSH, remediation, or fleet comparison without authorization.
- Reporting a hypothesis as fact when logs only show correlation.
