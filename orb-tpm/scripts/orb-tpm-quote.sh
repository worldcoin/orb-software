#!/usr/bin/env bash
# orb-tpm-quote.sh <nonce_hex> [pcr_selection]
#
# Produces a tpm_quote job result JSON (§14.2 schema) on stdout.
# Called on demand by orb-jobs-agent when the backend requests a PCR quote.
#
# Arguments:
#   nonce_hex     — 32-byte nonce as hex string (64 hex chars), backend-generated
#   pcr_selection — tpm2_quote PCR selection string (default: sha256:0,1,2,3,4,5,6,7)
#
# Example:
#   orb-tpm-quote.sh abcd1234ef... sha256:0,1,2,3,4,5,6,7
#
# TPM2TOOLS_TCTI must be set by the caller or inherited from the environment.

set -euo pipefail

NONCE_HEX="${1:?ERROR: nonce_hex argument required}"
PCR_SEL="${2:-sha256:0,1,2,3,4,5,6,7}"
AK_HANDLE=0x81010003

OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

# tpm2_quote takes a binary qualifying-data file, not a hex string.
printf '%s' "$NONCE_HEX" | xxd -r -p > "$OUT/nonce.bin"

tpm2_quote \
    --key-context    "$AK_HANDLE" \
    --pcr-list       "$PCR_SEL" \
    --qualification  "$OUT/nonce.bin" \
    --message        "$OUT/quote.bin" \
    --signature      "$OUT/sig.bin" \
    --hash-algorithm sha256

# Output the §14.2 job result schema as JSON using python3 (Orb has no jq).
# quoted_b64 / signature_b64 are raw TPM binary structures — do NOT decompose.
NONCE_HEX="$NONCE_HEX" OUT="$OUT" python3 - <<'PY_EOF'
import os, base64, json
from datetime import datetime, timezone

nonce_hex = os.environ['NONCE_HEX']
out_dir   = os.environ['OUT']

nonce_b64     = base64.b64encode(bytes.fromhex(nonce_hex)).decode()
quoted_b64    = base64.b64encode(open(out_dir + '/quote.bin', 'rb').read()).decode()
signature_b64 = base64.b64encode(open(out_dir + '/sig.bin',   'rb').read()).decode()

print(json.dumps({
    'schema_version': 1,
    'nonce_b64':      nonce_b64,
    'quoted_b64':     quoted_b64,
    'signature_b64':  signature_b64,
    'timestamp':      datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
}))
