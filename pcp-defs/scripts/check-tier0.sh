#!/usr/bin/env bash
# Checks an unencrypted PCP tier0 (orb-core `not-prod-pcp-export` + `not-prod-pcp-no-encrypt`).
# With two packages, also diffs each JSON file's shape; values differ per signup.
# Usage: check-tier0.sh <tier0.tar.gz> [other-tier0.tar.gz]
set -o errexit -o nounset -o pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
	echo "usage: $0 <tier0.tar.gz> [other-tier0.tar.gz]" >&2
	exit 2
fi

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }

# Extracted files hold raw biometrics; never leave them behind.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

status=0
for i in $(seq 1 $#); do
	pkg=${!i}
	mkdir "$work/$i"
	tar -xzf "$pkg" -C "$work/$i"
	echo "== $pkg"
	for f in "$work/$i"/*.json; do
		# The orb writes compact JSON with sorted keys; anything else changes the signed bytes.
		if cmp -s "$f" <(jq -cSj . "$f"); then
			echo "ok    sorted+compact  ${f##*/}"
		else
			echo "FAIL  not sorted+compact  ${f##*/}"
			status=1
		fi
	done
	if jq -r 'to_entries[] | select(.key | endswith(".json")) | "\(.value)  \(.key)"' \
		"$work/$i/hashes.json" | (cd "$work/$i" && sha256 -c --quiet -); then
		echo "ok    hashes.json matches every json file"
	else
		echo "FAIL  hashes.json does not match the files"
		status=1
	fi
done

if [ $# -eq 2 ]; then
	echo "== shape diff"
	shape='walk(if type == "string" then "string(\(length))" elif type == "number" then "number"
		elif type == "boolean" then "bool" else . end)'
	for f in "$work/1"/*.json; do
		name=${f##*/}
		if [ ! -f "$work/2/$name" ]; then
			echo "FAIL  only in first   $name"
			status=1
		elif diff <(jq "$shape" "$f") <(jq "$shape" "$work/2/$name"); then
			echo "ok    same shape      $name"
		else
			echo "FAIL  shape differs   $name"
			status=1
		fi
	done
	for f in "$work/2"/*.json; do
		[ -f "$work/1/${f##*/}" ] || {
			echo "FAIL  only in second  ${f##*/}"
			status=1
		}
	done
fi

exit $status
