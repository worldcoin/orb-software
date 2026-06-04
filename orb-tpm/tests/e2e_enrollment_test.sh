#!/usr/bin/env bash
# e2e_enrollment_test.sh
#
# End-to-end test: EK/AK creation → AK cert enrollment → TPM2 quote → backend verify.
#
# Runs entirely inside Docker (enrollment-runner image) with:
#   - tpm-sim (swtpm)            reachable via TPM2TOOLS_TCTI
#   - attestation-backend (Python server) reachable via BACKEND_URL
#
# Exit codes:
#   0  all phases passed
#   1  any phase failed (error printed to stderr)
#
# Environment variables:
#   BACKEND_URL       http://attestation-backend:8080
#   DEVICE_ID         test-device-001
#   TPM2TOOLS_TCTI    swtpm:host=tpm-sim,port=2321
#   STATE_DIR         /run/orb-tpm

set -euo pipefail

BACKEND_URL="${BACKEND_URL:-http://attestation-backend:8080}"
DEVICE_ID="${DEVICE_ID:-test-device-001}"
STATE_DIR="${STATE_DIR:-/run/orb-tpm}"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

pass() { echo -e "${GREEN}[PASS]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*" >&2; exit 1; }
step() { echo -e "${CYAN}───── $* ${NC}"; }

mkdir -p "$STATE_DIR"

# ── Phase 0: Wait for backend ──────────────────────────────────────────────────
step "Phase 0: Wait for attestation backend"
for i in $(seq 1 30); do
    if curl -sf "$BACKEND_URL/health" -o /dev/null 2>/dev/null; then
        pass "Backend is reachable at $BACKEND_URL"
        break
    fi
    echo "  Waiting for backend... ($i/30)"
    sleep 1
    if [[ $i -eq 30 ]]; then
        fail "Backend did not become ready within 30 seconds"
    fi
done

# ── Phase 0.5: Clear simulator DA lockout (swtpm only) ─────────────────────
# Reused swtpm state can retain DA lockout across runs and cause
# ActivateCredential to fail with TPM_RC_LOCKOUT (0x921).
if [[ "${TPM2TOOLS_TCTI:-}" == swtpm:* ]]; then
    step "Phase 0.5: Clear DA lockout (swtpm preflight)"
    if tpm2_dictionarylockout -c l --clear-lockout 2>/dev/null; then
        # Raise DA threshold in simulator to avoid lockout flakiness across
        # repeated local test runs and transient auth mismatches.
        tpm2_dictionarylockout -s -n 32 -t 1 -l 1 >/dev/null 2>&1 || true
        pass "Cleared DA lockout state and reset DA parameters"
    else
        echo "  DA clear skipped (lockout not active or hierarchy auth differs)"
    fi
fi

# ── Phase 1: EK/AK enrollment ──────────────────────────────────────────────────
step "Phase 1: EK/AK creation and AK cert enrollment"

if ! BACKEND_URL="$BACKEND_URL" DEVICE_ID="$DEVICE_ID" /usr/local/bin/orb-tpm-provision; then
    fail "orb-tpm-provision failed"
fi

pass "Enrollment completed"

# Recreate STATE_DIR — orb-tpm-provision's EXIT trap removes it on exit.
mkdir -p "$STATE_DIR"

# ── Phase 2: Verify enrollment state ──────────────────────────────────────────
step "Phase 2: Verify AK cert is in TPM NV and backend has it registered"

AK_CERT_NV=0x01800003
AK_HANDLE=0x81010003

# Check NV
if ! tpm2_nvread -C o "$AK_CERT_NV" -o "$STATE_DIR/post_enroll_cert.der" 2>/dev/null; then
    fail "AK cert not found in TPM NV at $AK_CERT_NV after enrollment"
fi
AK_CERT_SIZE=$(wc -c < "$STATE_DIR/post_enroll_cert.der")
[[ "$AK_CERT_SIZE" -gt 0 ]] || fail "AK cert in NV is empty"
pass "AK cert present in NV ($AK_CERT_SIZE bytes)"

# Check AK handle
if ! tpm2_readpublic -c "$AK_HANDLE" -o "$STATE_DIR/ak_post.pub" 2>/dev/null; then
    fail "AK key not found at persistent handle $AK_HANDLE"
fi
pass "AK key present at persistent handle $AK_HANDLE"

# Check backend registration
AK_PUB_B64="$(base64 -w0 "$STATE_DIR/ak_post.pub")"
HTTP_STATUS="$(curl -sf \
    -o "$STATE_DIR/status_response.json" \
    -w "%{http_code}" \
    -X GET "$BACKEND_URL/v1/attestation/ak/status" \
    -H "Content-Type: application/json" \
    -d "{\"device_id\":\"$DEVICE_ID\",\"ak_pub_b64\":\"$AK_PUB_B64\"}" 2>/dev/null)" || true

if [[ "$HTTP_STATUS" != "200" ]]; then
    fail "Backend returned HTTP $HTTP_STATUS for status check (expected 200)"
fi
pass "Backend confirmed AK registration (HTTP 200)"

# ── Phase 3: On-demand TPM2 quote ─────────────────────────────────────────────
step "Phase 3: Generate on-demand TPM2 quote"

# Generate a 32-byte random nonce (hex)
NONCE_HEX="$(python3 -c "import secrets; print(secrets.token_hex(32))")"
echo "  Nonce: $NONCE_HEX"

QUOTE_JSON="$(/usr/local/bin/orb-tpm-quote "$NONCE_HEX" "sha256:0,1,2,3,4,5,6,7")"
echo "  Quote JSON received ($(echo "$QUOTE_JSON" | wc -c) bytes)"

# Sanity-check the JSON structure (export first so nested python can read it)
export _QUOTE_JSON="$QUOTE_JSON"
python3 - <<'PY_EOF'
import sys, json, os
d = json.loads(os.environ['_QUOTE_JSON'])
assert d.get('schema_version') == 1, "missing schema_version"
assert d.get('quoted_b64'),          "missing quoted_b64"
assert d.get('signature_b64'),       "missing signature_b64"
assert d.get('nonce_b64'),           "missing nonce_b64"
print(f"  schema_version: {d['schema_version']}")
print(f"  quoted_b64 len: {len(d['quoted_b64'])}")
print(f"  signature_b64 len: {len(d['signature_b64'])}")
PY_EOF

pass "Quote JSON structure valid"

# ── Phase 4: Backend quote verification ───────────────────────────────────────
step "Phase 4: Send quote to backend for verification"

# Build the verify request JSON using Python (avoids shell quoting issues)
VERIFY_REQ="$(echo "$QUOTE_JSON" | python3 -c "
import sys, json
quote = json.load(sys.stdin)
req = {
    'device_id':     '${DEVICE_ID}',
    'nonce_hex':     '${NONCE_HEX}',
    'quoted_b64':    quote['quoted_b64'],
    'signature_b64': quote['signature_b64'],
}
print(json.dumps(req))
")"

VERIFY_RESPONSE="$(curl -sf -X POST "$BACKEND_URL/v1/attestation/quote/verify" \
    -H "Content-Type: application/json" \
    -d "$VERIFY_REQ")" || {
    fail "curl to /v1/attestation/quote/verify failed"
}

echo "  Verify response: $VERIFY_RESPONSE"

# Check the response (export first so Python can read it)
export _VERIFY_RESPONSE="$VERIFY_RESPONSE"
python3 - <<'PY_EOF'
import sys, json, os
resp = json.loads(os.environ['_VERIFY_RESPONSE'])
if not resp.get('valid'):
    print(f"[FAIL] quote verification returned: {resp}", file=sys.stderr)
    sys.exit(1)
print(f"  detail: {resp.get('detail', '')}")
PY_EOF

pass "Backend verified the TPM2 quote successfully"

# ── Phase 5: Idempotency check ────────────────────────────────────────────────
step "Phase 5: Re-run enrollment (idempotency — should be a no-op fast exit)"

if ! BACKEND_URL="$BACKEND_URL" DEVICE_ID="$DEVICE_ID" /usr/local/bin/orb-tpm-provision; then
    fail "Second orb-tpm-provision run failed"
fi
pass "Idempotent re-provision succeeded"

# ── Done ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}══════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  ✓  All e2e attestation tests PASSED             ${NC}"
echo -e "${GREEN}══════════════════════════════════════════════════${NC}"
