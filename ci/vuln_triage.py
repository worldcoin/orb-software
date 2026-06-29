#!/usr/bin/env python3
"""Rank + lane-route grype findings into a prioritized bump-PR queue.

Pure and offline: grype JSON in, queue JSON out. Opens no PRs, touches no
hardware. Lanes (first match wins): nvidia (BSP/kernel, manual review),
first (orb-*/worldcoin-* crates), third (everything else).
"""

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass

try:
    import tomllib
except ImportError:
    import tomli as tomllib  # type: ignore


def stderr(s):
    print(s, file=sys.stderr)


NVIDIA_PATTERNS = [
    r"\bnvidia\b",
    r"\btegra\b",
    r"\bl4t\b",
    r"\bjetson\b",
    r"\bnvgpu\b",
    r"\bnvmap\b",
    r"\bnvargus\b",
    r"\bnvsci\b",
    r"\bcuda\b",
    r"^linux(-.*)?$",
    r"\bkernel\b",
]
FIRST_PARTY_PREFIXES = ["orb-", "worldcoin-"]
SEVERITY_RANK = {
    "critical": 5,
    "high": 4,
    "medium": 3,
    "low": 2,
    "negligible": 1,
    "unknown": 0,
}
SEVERITY_SCORE = {
    "critical": 9.0,
    "high": 7.5,
    "medium": 5.0,
    "low": 3.0,
    "negligible": 1.0,
    "unknown": 0.0,
}
KEV_MULTIPLIER = 2.0
PRERELEASE = re.compile(r"(?i)(rc|alpha|beta|dev|pre|snapshot)\.?-?\d*")


def _load_json(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        raise SystemExit(f"FATAL: cannot read {path!r}: {e}")


def _num(x):
    try:
        return float(x)
    except (TypeError, ValueError):
        return 0.0


def _seg(part):
    return [
        (0, int(p)) if p.isdigit() else (1, p) for p in re.split(r"[.\-+~]", part) if p
    ]


def _vkey(v):
    # Normalize go/v prefixes so 'go1.23.3' and 'v1.2.0' compare numerically.
    v = re.sub(r"^(v|go)", "", v)
    # Split off a prerelease tag so it sorts BELOW its release (1.2.0rc1 < 1.2.0).
    m = PRERELEASE.search(v)
    rel, pre = (v[: m.start()].rstrip(".-+~"), v[m.start() :]) if m else (v, "")
    return (_seg(rel), (0, _seg(pre)) if pre else (1, []))


def _is_pre(v):
    return bool(PRERELEASE.search(v))


@dataclass
class Finding:
    id: str
    aliases: list
    severity: str
    cvss: float
    package: str
    version: str
    pkg_type: str
    purl: str
    fixes: list
    fix_state: str
    url: str
    platforms: list

    @property
    def has_fix(self):
        return self.fix_state == "fixed" and bool(self.fixes)

    @property
    def all_ids(self):
        return [self.id, *self.aliases]


def parse_grype(path, platform):
    doc = _load_json(path)
    if "matches" not in doc:
        raise SystemExit(f"FATAL: {path!r} is not grype JSON (use -o json)")
    findings = []
    for match in doc["matches"]:
        vuln = match.get("vulnerability") or {}
        artifact = match.get("artifact") or {}
        fix = vuln.get("fix") or {}
        cvss = max(
            _num(((metric or {}).get("metrics") or {}).get("baseScore"))
            for metric in (vuln.get("cvss") or [{}])
        )
        findings.append(
            Finding(
                id=vuln.get("id", "UNKNOWN"),
                aliases=[
                    related["id"]
                    for related in (match.get("relatedVulnerabilities") or [])
                    if related.get("id")
                ],
                severity=(vuln.get("severity") or "unknown").lower(),
                cvss=cvss,
                package=artifact.get("name", ""),
                version=artifact.get("version", ""),
                pkg_type=artifact.get("type", ""),
                purl=artifact.get("purl", ""),
                fixes=list(fix.get("versions") or []),
                fix_state=fix.get("state", "unknown"),
                url=vuln.get("dataSource", ""),
                platforms=[platform],
            )
        )
    return findings


def load_ignored_ids(path):
    if not path:
        return set()
    try:
        with open(path, "rb") as f:
            doc = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise SystemExit(f"FATAL: cannot read {path!r}: {e}")
    return {
        e["id"] if isinstance(e, dict) else e
        for e in ((doc.get("advisories") or {}).get("ignore") or [])
        if (isinstance(e, dict) and e.get("id")) or isinstance(e, str)
    }


def load_kev_ids(path):
    # Optional enrichment: a missing/empty/corrupt catalog must degrade, not abort.
    if not path:
        return set()
    try:
        with open(path) as f:
            doc = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        stderr(f"WARN: ignoring unreadable KEV catalog {path!r}: {e}")
        return set()
    return {v["cveID"] for v in (doc.get("vulnerabilities") or []) if v.get("cveID")}


def route_lane(f, prefixes):
    # Cargo crates are never kernel/BSP, so the nvidia patterns must not apply to
    # them (e.g. the crate `linux-raw-sys`); route them by first/third only.
    is_cargo = f.pkg_type in ("rust-crate", "rust") or "pkg:cargo/" in f.purl
    if is_cargo:
        return "first" if any(f.package.startswith(p) for p in prefixes) else "third"
    # Non-cargo: match name and purl separately so anchored patterns (^linux$) work.
    name, purl = f.package.lower(), f.purl.lower()
    if any(re.search(p, name) or re.search(p, purl) for p in NVIDIA_PATTERNS):
        return "nvidia"
    return "third"


def pick_target(findings, current):
    # Per CVE take the smallest stable fix ahead of current; across CVEs take
    # the max (the minimal single version that clears them all). Prereleases
    # are used only when no stable fix exists.
    cur = _vkey(current) if current else None
    picks = []
    for f in findings:
        stable = [v for v in f.fixes if not _is_pre(v)] or f.fixes
        ahead = [v for v in stable if cur and _vkey(v) > cur]
        picks.append(min(ahead, key=_vkey) if ahead else max(stable, key=_vkey))
    return max(picks, key=_vkey)


def dedup(findings):
    merged = {}
    for f in findings:
        key = (f.id, f.package, f.version)
        if key in merged:
            merged[key].platforms = sorted(
                set(merged[key].platforms) | set(f.platforms)
            )
        else:
            merged[key] = f
    return list(merged.values())


def build_item(lane, package, findings, kev_ids):
    current = sorted({f.version for f in findings if f.version}, key=_vkey)
    target = pick_target(findings, current[-1] if current else "")
    platforms = sorted({p for f in findings for p in f.platforms})
    kev = any(i in kev_ids for f in findings for i in f.all_ids)
    score = round(
        max(
            (f.cvss or SEVERITY_SCORE[f.severity])
            * (KEV_MULTIPLIER if any(i in kev_ids for i in f.all_ids) else 1.0)
            for f in findings
        ),
        2,
    )
    severity = max((f.severity for f in findings), key=lambda s: SEVERITY_RANK[s])
    cves = sorted(
        (
            {
                "id": f.id,
                "severity": f.severity,
                "cvss": f.cvss,
                "kev": any(i in kev_ids for i in f.all_ids),
                "fixes": f.fixes,
                "url": f.url,
            }
            for f in findings
        ),
        key=lambda c: SEVERITY_RANK[c["severity"]],
        reverse=True,
    )

    review = lane == "nvidia"
    labels = ["security", "automated-bump", f"lane:{lane}"]
    if review:
        labels.append("manual-review-required")
    if kev:
        labels.append("known-exploited")

    rows = "\n".join(
        f"| {f'[{c['id']}]({c['url']})' if c['url'] else c['id']} | {c['severity']} | "
        f"{c['cvss'] or '—'} | {'⚠️' if c['kev'] else ''} | {', '.join(c['fixes']) or '—'} |"
        for c in cves
    )
    warn = (
        "> [!WARNING]\n> NVIDIA/BSP/kernel — mandatory human review; verify "
        "upstream changelog and run full HIL on both platforms.\n\n"
        if review
        else ""
    )
    body = (
        f"{warn}Security bump for **{package}** "
        f"`{', '.join(current) or '?'}` → `{target}` ({', '.join(platforms)}).\n\n"
        f"| Advisory | Sev | CVSS | KEV | Fixed in |\n| --- | --- | --- | --- | --- |\n{rows}"
    )

    return {
        "lane": lane,
        "package": package,
        "pkg_type": findings[0].pkg_type,
        "current_versions": current,
        "target_version": target,
        "score": score,
        "severity": severity,
        "kev": kev,
        "platforms": platforms,
        "cves": cves,
        "branch": re.sub(
            r"[^a-z0-9._/-]+",
            "-",
            f"vuln/{lane}/{package}/{findings[0].version or 'x'}".lower(),
        ).strip("-/"),
        "title": f"fix(deps): bump {package} to {target} ({len(cves)} advisor{'y' if len(cves) == 1 else 'ies'})",
        "body": body,
        "labels": labels,
        "action": "open",
    }


def subcmd_triage(args):
    if len(args.grype) != len(args.platform):
        raise SystemExit("FATAL: --grype and --platform must be paired 1:1")

    ignored = load_ignored_ids(args.deny_toml)
    kev_ids = load_kev_ids(args.kev_catalog)
    prefixes = args.first_party_prefix or FIRST_PARTY_PREFIXES
    existing = set()
    if args.existing_branches and os.path.exists(args.existing_branches):
        existing = {ln.strip() for ln in open(args.existing_branches) if ln.strip()}

    findings = dedup(
        [
            f
            for path, plat in zip(args.grype, args.platform)
            for f in parse_grype(path, plat)
        ]
    )

    by_sev, groups, parked = {}, {}, []
    for f in findings:
        by_sev[f.severity] = by_sev.get(f.severity, 0) + 1
        if any(i in ignored for i in f.all_ids):
            continue
        if not f.has_fix:
            parked.append(
                {
                    "id": f.id,
                    "package": f.package,
                    "version": f.version,
                    "severity": f.severity,
                    "fix_state": f.fix_state,
                }
            )
            continue
        # Group by version too: duplicate crate versions are separate semver
        # lines and must each get their own (correct) bump target.
        groups.setdefault((route_lane(f, prefixes), f.package, f.version), []).append(f)

    items = [
        build_item(lane, pkg, fs, kev_ids) for (lane, pkg, _ver), fs in groups.items()
    ]
    # A target that is not strictly newer than the installed version is a no-op or
    # downgrade (e.g. grype namespace quirks); park it instead of proposing a bump.
    upgrades = []
    for it in items:
        cur = it["current_versions"]
        if cur and _vkey(it["target_version"]) <= _vkey(cur[-1]):
            parked += [
                {
                    "id": c["id"],
                    "package": it["package"],
                    "version": cur[-1],
                    "severity": c["severity"],
                    "fix_state": "no-newer-fix",
                }
                for c in it["cves"]
            ]
        else:
            upgrades.append(it)
    items = upgrades

    items.sort(
        key=lambda i: (
            i["kev"],
            i["score"],
            SEVERITY_RANK[i["severity"]],
            i["package"],
        ),
        reverse=True,
    )

    to_open = dropped = 0
    for i in items:
        if i["branch"] in existing:
            i["action"] = "already_open"
        elif args.max_prs and to_open >= args.max_prs:
            i["action"] = "dropped_over_cap"
            dropped += 1
        else:
            to_open += 1
    if dropped:
        stderr(
            f"NOTE: {dropped} item(s) exceed --max-prs={args.max_prs}, deferred to next run"
        )

    by_lane = {}
    for i in items:
        by_lane[i["lane"]] = by_lane.get(i["lane"], 0) + 1
    summary = {
        "total_findings": len(findings),
        "ignored_waived": sum(
            1 for f in findings if any(x in ignored for x in f.all_ids)
        ),
        "parked_no_fix": len(parked),
        "work_items": len(items),
        "to_open": to_open,
        "already_open": sum(1 for i in items if i["action"] == "already_open"),
        "dropped_over_cap": dropped,
        "by_lane": by_lane,
        "by_severity": by_sev,
    }

    if args.out:
        open(args.out, "w").write(
            json.dumps({"summary": summary, "parked": parked, "items": items}, indent=2)
        )
    print(_table(summary, items))
    stderr("METRICS " + json.dumps({"vuln_triage": summary}))


def _table(summary, items):
    lines = [
        f"{summary['work_items']} bumps | parked {summary['parked_no_fix']} | "
        f"waived {summary['ignored_waived']}",
        "| Sev | Package | Target | CVEs |",
        "| --- | --- | --- | --- |",
    ]
    for i in items:
        if i["action"] == "dropped_over_cap":
            continue
        sev = ("!" if i["kev"] else "") + i["severity"]
        lines.append(
            f"| {sev} | {i['package']} | {i['target_version']} | {len(i['cves'])} |"
        )
    return "\n".join(lines)


def main():
    p = argparse.ArgumentParser(description="Vulnerability-driven bump triage")
    sub = p.add_subparsers(required=True)

    t = sub.add_parser("triage", help="grype JSON -> ranked queue JSON")
    t.add_argument("--grype", action="append", required=True, metavar="FILE")
    t.add_argument("--platform", action="append", required=True)
    t.add_argument("--deny-toml", help="honor its [advisories].ignore waivers")
    t.add_argument("--kev-catalog", help="CISA KEV JSON for exploit boosting")
    t.add_argument("--first-party-prefix", action="append")
    t.add_argument(
        "--existing-branches", help="file of branch names to mark already_open"
    )
    t.add_argument("--max-prs", type=int, default=0, help="0 = unlimited")
    t.add_argument("--out")
    t.set_defaults(entry_point=subcmd_triage)

    args = p.parse_args()
    args.entry_point(args)


if __name__ == "__main__":
    main()
