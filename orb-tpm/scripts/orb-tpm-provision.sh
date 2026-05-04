#!/usr/bin/env bash
# orb-tpm-provision.sh
#
# Idempotent EK/AK provisioning for the Orb OP-TEE fTPM.
# Runs every boot via orb-tpm.worldcoin-tpm-provision.service
# (After=ftpm_is_ready.target).
#
# No sentinel file is used: /usr/persist is flaky.
# Fast-path: if AK cert is in TPM NV AND backend confirms the AK is
# registered, exit 0 immediately (~1 s).  Re-enrollment is triggered
# automatically whenever either check fails.
#
# Reference:
#   https://tpm2-software.github.io/2020/06/12/Remote-Attestation-With-tpm2-tools.html
#
# TPM2TOOLS_TCTI must be set by the caller (systemd unit sets it to
# device:/dev/optee_ftpmrm).

set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────

BACKEND_URL="${BACKEND_URL:-https://attestation.worldcoin.dev}"
EK_HANDLE=0x81010001   # TCG EK persistent handle (ECC P-256)
AK_HANDLE=0x81010003   # AIK persistent handle
AK_CERT_NV=0x01800003  # AK cert NV index — owner-defined range (0x01800000–0x01BFFFFF)
                        # 0x01C000xx is TCG-reserved for EK certs; do not use that range.
STATE_DIR=/run/orb-tpm  # tmpfs — only needed during this boot's provisioning
AK_PUB="$STATE_DIR/ak.pub"
AK_NAME="$STATE_DIR/ak.name"

DEVICE_ID="$(cat /proc/device-tree/serial-number 2>/dev/null \
             || cat /etc/orb-id \
             || { echo "[orb-tpm] ERROR: cannot determine device ID" >&2; exit 1; })"

mkdir -p "$STATE_DIR"  # no-op when managed by systemd RuntimeDirectory
trap 'rm -rf "$STATE_DIR"' EXIT

log() { echo "[orb-tpm] $*"; }
err() { echo "[orb-tpm] ERROR: $*" >&2; }

# ── Fast-path: check if already provisioned ───────────────────────────────────
#
# Three-way check:
#  1. AK cert is readable from TPM NV.
#  2. Backend has this AK registered (GET /status returns 200).
#  3. The cert fingerprint in NV matches the fingerprint the backend holds.
#
# Fingerprint comparison catches desync: e.g. the NV still has an old cert
# from a previous AK, or the backend was restored to a different registration.

ak_cert_matches_backend() {
    tpm2_nvread "$AK_CERT_NV" -o "$STATE_DIR/ak_cert_nv.der" 2>/dev/null || return 1

    local nv_fp
    nv_fp="$(openssl dgst -sha256 -binary "$STATE_DIR/ak_cert_nv.der" \
             | python3 -c 'import sys; print(sys.stdin.buffer.read().hex(), end="")')" || return 1

    tpm2_readpublic -c "$AK_HANDLE" -o "$AK_PUB" 2>/dev/null || return 1
    local ak_pub_b64
    ak_pub_b64="$(base64 -w0 "$AK_PUB")"

    local http_status
    http_status="$(curl -sf \
        -o "$STATE_DIR/status_response.json" \
        -w "%{http_code}" \
        -X GET "$BACKEND_URL/v1/attestation/ak/status" \
        -H "Content-Type: application/json" \
        -d "{\"device_id\":\"$DEVICE_ID\",\"ak_pub_b64\":\"$ak_pub_b64\"}")" || return 1

    [[ "$http_status" == "200" ]] || return 1

    local backend_fp
    backend_fp="$(STATE_DIR="$STATE_DIR" python3 - <<'PY_EOF'
import os, json
d = json.load(open(os.environ['STATE_DIR'] + '/status_response.json'))
print(d['ak_cert_fingerprint_sha256'], end='')
PY_EOF
    )" || return 1

    [[ "$nv_fp" == "$backend_fp" ]]
}

if ak_cert_matches_backend; then
    log "AK cert in NV matches backend registration — nothing to do."
    exit 0
fi

log "Provisioning required — starting EK/AK setup..."

# ── Phase 1: EK ───────────────────────────────────────────────────────────────
# tpm2_createek applies the TCG EK Credential Profile template (ECC P-256).
# The EK is deterministic: same fTPM seed always produces the same key.

if tpm2_readpublic -c "$EK_HANDLE" -o "$STATE_DIR/ek.pub" 2>/dev/null; then
    log "EK already persisted at $EK_HANDLE"
else
    log "Creating EK..."
    tpm2_createek \
        --ek-context    "$STATE_DIR/ek.ctx" \
        --key-algorithm ecc \
        --public        "$STATE_DIR/ek.pub"
    tpm2_evictcontrol -c "$STATE_DIR/ek.ctx" "$EK_HANDLE"
    rm -f "$STATE_DIR/ek.ctx"
    log "EK persisted at $EK_HANDLE"
fi

# ── Phase 2: AK ───────────────────────────────────────────────────────────────
# tpm2_createak creates an ECC P-256 restricted signing key under the EK.
# The AK lives in the endorsement hierarchy; loading it requires
# PolicySecret(Endorsement) — tpm2_activatecredential handles this automatically.
# In Rust (tss-esapi): use tss_esapi::abstraction::ak::{create_ak, load_ak}.

AK_NEEDS_ENROLLMENT=false

if tpm2_readpublic -c "$AK_HANDLE" -o "$AK_PUB" -n "$AK_NAME" 2>/dev/null; then
    log "AK already persisted at $AK_HANDLE"
else
    log "Creating AK..."
    tpm2_createak \
        --ek-context        "$EK_HANDLE" \
        --ak-context        "$STATE_DIR/ak.ctx" \
        --key-algorithm     ecc \
        --hash-algorithm    sha256 \
        --signing-algorithm ecdsa \
        --public            "$AK_PUB" \
        --private           "$STATE_DIR/ak.priv" \
        --ak-name           "$AK_NAME"
    tpm2_evictcontrol -c "$STATE_DIR/ak.ctx" "$AK_HANDLE"
    rm -f "$STATE_DIR/ak.ctx" "$STATE_DIR/ak.priv"
    AK_NEEDS_ENROLLMENT=true
    log "AK persisted at $AK_HANDLE"
fi

if [[ ! -f "$AK_NAME" ]]; then
    tpm2_readpublic -c "$AK_HANDLE" -o "$AK_PUB" -n "$AK_NAME"
fi

# ── Phase 3: AK cert enrollment ───────────────────────────────────────────────
# Always run enrollment if:
#   - AK was just created (new random key → definitely not registered)
#   - AK cert is missing from NV
#   - Backend did not confirm registration (fast-path check above failed)

log "Starting AK enrollment (ActivateCredential)..."

EK_PUB_B64="$(base64 -w0 "$STATE_DIR/ek.pub")"
AK_PUB_B64="$(base64 -w0 "$AK_PUB")"
AK_NAME_HEX="$(xxd -p -c 256 "$AK_NAME")"

# Step 1: Request MakeCredential challenge from backend.
# Security: the backend generates the secret; only ActivateCredential on this
# specific fTPM (holding the matching EK private key) can recover it.
log "Requesting challenge from backend..."
CHALLENGE="$(curl -sf -X POST "$BACKEND_URL/v1/attestation/ak/challenge" \
    -H "Content-Type: application/json" \
    -d "{\"device_id\":   \"$DEVICE_ID\",
         \"ek_pub_b64\":  \"$EK_PUB_B64\",
         \"ak_pub_b64\":  \"$AK_PUB_B64\",
         \"ak_name_hex\": \"$AK_NAME_HEX\"}")"

_CHALLENGE="$CHALLENGE" STATE_DIR="$STATE_DIR" python3 - <<'PY_EOF'
import sys, json, base64, os
d = json.loads(os.environ['_CHALLENGE'])
open(os.environ['STATE_DIR'] + '/cred.blob',  'wb').write(base64.b64decode(d['credential_blob_b64']))
open(os.environ['STATE_DIR'] + '/enc.secret', 'wb').write(base64.b64decode(d['encrypted_secret_b64']))
PY_EOF

# Step 2: ActivateCredential.
# tpm2_activatecredential automatically opens a PolicySecret(Endorsement) session
# to satisfy the EK authorization policy — no manual tpm2_startauthsession needed.
log "Running ActivateCredential..."
tpm2_activatecredential \
    --credentialedkey-context "$AK_HANDLE" \
    --credentialkey-context   "$EK_HANDLE" \
    --credential-blob         "$STATE_DIR/cred.blob" \
    --certinfo-data           "$STATE_DIR/recovered_secret.bin"

SECRET_B64="$(base64 -w0 "$STATE_DIR/recovered_secret.bin")"

# Step 3: Return recovered secret; backend verifies and issues AK cert.
log "Completing enrollment with backend..."
curl -sf -X POST "$BACKEND_URL/v1/attestation/ak/complete" \
    -H "Content-Type: application/json" \
    -d "{\"device_id\":  \"$DEVICE_ID\",
         \"secret_b64\": \"$SECRET_B64\"}" \
    -o "$STATE_DIR/complete_response.json"

STATE_DIR="$STATE_DIR" python3 - <<'PY_EOF'
import sys, json, base64, os
d = json.load(open(os.environ['STATE_DIR'] + '/complete_response.json'))
if not d.get('ak_cert_b64'):
    sys.exit('[orb-tpm] ERROR: backend returned empty AK cert — enrollment failed.')
open(os.environ['STATE_DIR'] + '/ak_cert.der', 'wb').write(base64.b64decode(d['ak_cert_b64']))
PY_EOF

tpm2_nvwrite "$AK_CERT_NV" -i "$STATE_DIR/ak_cert.der"

log "Enrollment complete. AK cert stored at NV $AK_CERT_NV"
