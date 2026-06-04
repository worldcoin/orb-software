#!/usr/bin/env python3
"""
Minimal fTPM attestation backend for local testing.

State is in-memory only — no database, no persistence across restarts.
Suitable for Orb-to-PC end-to-end enrollment tests.

Implements the three endpoints consumed by orb-tpm-provision.sh:
    POST /v1/attestation/ak/challenge
    POST /v1/attestation/ak/complete
    GET  /v1/attestation/ak/status

MakeCredential is delegated to `tpm2_makecredential --tcti none` (tpm2-tools)
which runs as a pure-crypto operation without a physical TPM.

Requirements:
    apt install tpm2-tools
    pip install cryptography

Usage (Docker):
    docker compose up

Usage (direct):
    python3 orb-attestation-test-server.py [--host 0.0.0.0] [--port 8080]

    On Orb:
        BACKEND_URL=http://<your-PC-IP>:8080
"""

import argparse
import base64
import datetime
import hashlib
import hmac
import json
import os
import secrets
import struct
import subprocess
import sys
import tempfile
from http.server import BaseHTTPRequestHandler, HTTPServer

try:
    from cryptography.hazmat.primitives.asymmetric import ec
    from cryptography.hazmat.primitives.asymmetric.ec import (
        EllipticCurvePublicKey, SECP256R1
    )
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.backends import default_backend
    from cryptography import x509
    from cryptography.x509.oid import NameOID
except ImportError:
    sys.exit("ERROR: pip install cryptography")


# ── In-memory store ───────────────────────────────────────────────────────────
# {device_id: {"ek_pub": bytes, "ak_pub": bytes, "ak_name": bytes,
#              "secret": bytes,
#              "ak_cert_der": bytes, "ak_cert_fp_sha256": str}}
store: dict[str, dict] = {}

# ── CA key — ephemeral, regenerated each run ──────────────────────────────────
CA_KEY = ec.generate_private_key(SECP256R1(), default_backend())
CA_CERT = (
    x509.CertificateBuilder()
    .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Test-Attestation-CA")]))
    .issuer_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Test-Attestation-CA")]))
    .not_valid_before(datetime.datetime.utcnow())
    .not_valid_after(datetime.datetime.utcnow() + datetime.timedelta(days=365))
    .serial_number(x509.random_serial_number())
    .public_key(CA_KEY.public_key())
    .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
    .sign(CA_KEY, hashes.SHA256(), default_backend())
)


# ── TPM2B_PUBLIC parser (ECC P-256 — handles both EK and AK parameter layouts) ──
#
# EK template: symmetric=AES-128-CFB (algId+keyBits+mode = 6 bytes), scheme=NULL (2 bytes)
# AK template: symmetric=NULL        (algId only = 2 bytes),          scheme=ECDSA-SHA256 (4 bytes)
# Both:        curveID=NistP256 (2 bytes), kdf=NULL (2 bytes)

_TPM_ALG_AES       = 0x0006
_TPM_ALG_NULL      = 0x0010
_TPM_ALG_ECDSA     = 0x0018
_TPM_ALG_ECDH      = 0x0019
_TPM_ALG_ECDAA     = 0x001A
_TPM_ALG_SM2       = 0x001B
_TPM_ALG_ECSCHNORR = 0x001C
_TPM_ALG_MGF1      = 0x0007


def parse_tpm2b_public_ecc(data: bytes):
    """
    Return (x_bytes, y_bytes) from a TPM2B_PUBLIC ECC wire blob.
    Dynamically reads symmetric/scheme algIds so it works for both
    EK (AES-128-CFB, NULL scheme) and AK (NULL symmetric, ECDSA-SHA256).
    """
    offset = 2  # skip outer TPM2B size field (2 bytes)

    # TPMT_PUBLIC header: type(2) + nameAlg(2) + objectAttributes(4)
    offset += 2 + 2 + 4

    # authPolicy: TPM2B — size(2) + data(size bytes)
    auth_size = struct.unpack_from(">H", data, offset)[0]
    offset += 2 + auth_size

    # TPMS_ECC_PARMS — symmetric (TPMT_SYM_CIPHER):
    sym_alg = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    if sym_alg == _TPM_ALG_AES:
        offset += 2 + 2   # keyBits + mode
    elif sym_alg == _TPM_ALG_NULL:
        pass              # no additional fields
    else:
        raise ValueError(f"Unexpected symmetric algId 0x{sym_alg:04x}")

    # TPMS_ECC_PARMS — scheme (TPMT_ECC_SCHEME):
    scheme_alg = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    if scheme_alg == _TPM_ALG_NULL:
        pass
    elif scheme_alg in (_TPM_ALG_ECDSA, _TPM_ALG_ECDAA, _TPM_ALG_SM2,
                        _TPM_ALG_ECSCHNORR, _TPM_ALG_ECDH, _TPM_ALG_MGF1):
        offset += 2       # hashAlg / kdf
    else:
        raise ValueError(f"Unexpected scheme algId 0x{scheme_alg:04x}")

    # curveID (TPMI_ECC_CURVE): 2 bytes
    offset += 2

    # kdf (TPMT_KDF_SCHEME): algId(2) [+ hashAlg(2) if not NULL]
    kdf_alg = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    if kdf_alg != _TPM_ALG_NULL:
        offset += 2

    # TPMS_ECC_POINT (unique):
    x_size = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    x_bytes = data[offset: offset + x_size]
    offset += x_size
    y_size = struct.unpack_from(">H", data, offset)[0]
    offset += 2
    y_bytes = data[offset: offset + y_size]

    return x_bytes, y_bytes


def load_ec_pub_from_tpm2b(tpm2b: bytes) -> EllipticCurvePublicKey:
    x_bytes, y_bytes = parse_tpm2b_public_ecc(tpm2b)
    x = int.from_bytes(x_bytes, "big")
    y = int.from_bytes(y_bytes, "big")
    return ec.EllipticCurvePublicNumbers(x, y, SECP256R1()).public_key(default_backend())


# ── MakeCredential — pure-Python TPM2 implementation ─────────────────────────
#
# TPM2_MakeCredential is a pure cryptographic operation (no TPM hardware needed).
# Reference: TCG TPM2 Library Specification Part 1 §24.5
#
# Algorithm for ECC EK (P-256, SHA-256, AES-128-CFB):
#   1. Generate ephemeral ECDH key pair (d_eph, Q_eph)
#   2. Z = ECDH(d_eph, Q_ek).x  (shared secret x-coordinate)
#   3. seed = KDFe(SHA-256, Z, "IDENTITY", Q_eph.x, Q_ek.x, 256)
#   4. encryptedSecret = TPMS_ECC_POINT(Q_eph)
#   5. HMACkey  = KDFa(SHA-256, seed, "INTEGRITY", "", "", 256)
#   6. symKey   = KDFa(SHA-256, seed, "STORAGE", objectName, "", 128)
#   7. encCred  = AES-128-CFB(symKey, IV=0, credential)
#   8. mac      = HMAC-SHA-256(HMACkey, encCred || objectName)
#   9. credentialBlob = TPM2B_ID_OBJECT { integrityHMAC, encryptedCredential }
#
# Output format matches tpm2_makecredential binary output.

import hmac as _hmac
import hashlib as _hashlib


def _kdfa(key: bytes, label: str, context_u: bytes, context_v: bytes, bits: int) -> bytes:
    """TPM2 KDFa (HMAC-based KDF, §11.4.9.2)."""
    result = b""
    counter = 1
    label_b = label.encode("utf-8") + b"\x00"
    bits_b = bits.to_bytes(4, "big")
    while len(result) * 8 < bits:
        data = counter.to_bytes(4, "big") + label_b + context_u + context_v + bits_b
        result += _hmac.new(key, data, _hashlib.sha256).digest()
        counter += 1
    return result[:bits // 8]


def _kdfe(z_x: bytes, label: str, party_u_x: bytes, party_v_x: bytes, bits: int) -> bytes:
    """TPM2 KDFe (hash-based ECDH seed derivation, §11.4.9.3).
    Note: unlike KDFa, KDFe does NOT include `bits` as the final input field."""
    result = b""
    counter = 1
    label_b = label.encode("utf-8") + b"\x00"
    while len(result) * 8 < bits:
        data = counter.to_bytes(4, "big") + label_b + z_x + party_u_x + party_v_x
        result += _hashlib.sha256(data).digest()
        counter += 1
    return result[:bits // 8]


def make_credential(ek_pub_tpm2b: bytes, ak_name: bytes, secret: bytes) -> bytes:
    """
    Compute TPM2_MakeCredential via tpm2_makecredential subprocess.

    tpm2_makecredential (tpm2-tools 5.x) is a pure-crypto operation that
    produces the same output as the TPM command but runs locally.  It needs
    a TCTI connection only for initialization (no persistent TPM state is read
    or written when --encryption-key FILE is given instead of a handle).

    Returns the raw combined binary that tpm2_activatecredential reads with -i:
        Magic (4B) || Version (4B) || TPM2B_ID_OBJECT || TPM2B_ENCRYPTED_SECRET
    """
    with tempfile.TemporaryDirectory() as tmp:
        ek_pub_file    = os.path.join(tmp, "ek.pub")
        secret_file    = os.path.join(tmp, "secret.bin")
        cred_blob_file = os.path.join(tmp, "cred.blob")

        with open(ek_pub_file, "wb") as f: f.write(ek_pub_tpm2b)
        with open(secret_file, "wb") as f: f.write(secret)

        result = subprocess.run(
            [
                "tpm2_makecredential",
                "--encryption-key", ek_pub_file,
                "--name",           ak_name.hex(),
                "--secret",         secret_file,
                "--credential-blob", cred_blob_file,
            ],
            capture_output=True,
            text=True,
        )

        if result.returncode != 0:
            raise RuntimeError(
                f"tpm2_makecredential failed (rc={result.returncode}): "
                f"{result.stderr.strip()}"
            )

        with open(cred_blob_file, "rb") as f:
            return f.read()


# ── AK cert issuance ─────────────────────────────────────────────────────────

def issue_ak_cert(device_id: str, ak_pub_tpm2b: bytes) -> bytes:
    """Issue a self-signed (test CA) X.509 cert for the AK pub. Returns DER."""
    ak_pub_key = load_ec_pub_from_tpm2b(ak_pub_tpm2b)
    cert = (
        x509.CertificateBuilder()
        .subject_name(x509.Name([
            x509.NameAttribute(NameOID.COMMON_NAME, f"AK-{device_id}"),
        ]))
        .issuer_name(CA_CERT.subject)
        .not_valid_before(datetime.datetime.utcnow())
        .not_valid_after(datetime.datetime.utcnow() + datetime.timedelta(days=365))
        .serial_number(x509.random_serial_number())
        .public_key(ak_pub_key)
        .add_extension(
            x509.ExtendedKeyUsage([x509.oid.ObjectIdentifier("2.23.133.8.3")]),  # tcg-kp-AIKCertificate
            critical=False,
        )
        .sign(CA_KEY, hashes.SHA256(), default_backend())
    )
    return cert.public_bytes(serialization.Encoding.DER)


def cert_fingerprint(der: bytes) -> str:
    return hashlib.sha256(der).hexdigest()


# ── Quote verification ─────────────────────────────────────────────────────────
#
# Uses tpm2_checkquote (tpm2-tools, no TPM needed) to verify:
#   - the TPM2B_ATTEST signature against the AK public key
#   - the qualifying data (nonce) embedded in TPMT_ATTEST
#
# tpm2_checkquote reads:
#   -u key.pub  (TPM2B_PUBLIC format — the stored AK pub from enrollment)
#   -m quote.bin (TPM2B_ATTEST binary)
#   -s sig.bin  (TPMT_SIGNATURE binary)
#   -q nonce.bin (raw nonce bytes that should appear in extraData)

def verify_quote(
    ak_pub_tpm2b: bytes,
    quoted_bytes: bytes,
    signature_bytes: bytes,
    nonce_bytes: bytes,
) -> dict:
    """
    Run tpm2_checkquote and return {"valid": bool, "detail": str}.
    Raises on unexpected subprocess error.
    """
    with tempfile.TemporaryDirectory() as tmp:
        ak_pub_file  = os.path.join(tmp, "ak.pub")
        quote_file   = os.path.join(tmp, "quote.bin")
        sig_file     = os.path.join(tmp, "sig.bin")
        nonce_file   = os.path.join(tmp, "nonce.bin")

        with open(ak_pub_file,  "wb") as f: f.write(ak_pub_tpm2b)
        with open(quote_file,   "wb") as f: f.write(quoted_bytes)
        with open(sig_file,     "wb") as f: f.write(signature_bytes)
        with open(nonce_file,   "wb") as f: f.write(nonce_bytes)

        result = subprocess.run(
            [
                "tpm2_checkquote",
                "--public",        ak_pub_file,   # TPM2B_PUBLIC binary (tpm2_checkquote uses -u/--public)
                "--message",       quote_file,
                "--signature",     sig_file,
                "--qualification", nonce_file,
            ],
            capture_output=True,
            text=True,
        )

    if result.returncode == 0:
        return {"valid": True,  "detail": result.stdout.strip() or "ok"}
    else:
        return {"valid": False, "detail": result.stderr.strip() or "verification failed"}


# ── HTTP request handler ──────────────────────────────────────────────────────

class Handler(BaseHTTPRequestHandler):

    def log_message(self, fmt, *args):
        print(f"[server] {self.address_string()} {fmt % args}")

    def _body(self) -> dict:
        length = int(self.headers.get("Content-Length", 0))
        return json.loads(self.rfile.read(length)) if length else {}

    def _send(self, status: int, payload: dict):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        if self.path == "/v1/attestation/ak/challenge":
            self._challenge()
        elif self.path == "/v1/attestation/ak/complete":
            self._complete()
        elif self.path == "/v1/attestation/quote/verify":
            self._verify_quote()
        else:
            self._send(404, {"error": "not found"})

    def do_GET(self):
        if self.path == "/v1/attestation/ak/status":
            self._status()
        elif self.path == "/health":
            self._send(200, {"status": "ok"})
        else:
            self._send(404, {"error": "not found"})

    # POST /v1/attestation/ak/challenge
    def _challenge(self):
        body = self._body()
        device_id = body.get("device_id", "")
        ek_pub_b64 = body.get("ek_pub_b64", "")
        ak_pub_b64 = body.get("ak_pub_b64", "")
        ak_name_hex = body.get("ak_name_hex", "")

        if not all([device_id, ek_pub_b64, ak_pub_b64, ak_name_hex]):
            self._send(400, {"error": "missing fields"})
            return

        ek_pub_tpm2b = base64.b64decode(ek_pub_b64)
        ak_pub_tpm2b = base64.b64decode(ak_pub_b64)
        ak_name = bytes.fromhex(ak_name_hex)

        secret = secrets.token_bytes(32)

        store[device_id] = {
            "ek_pub_tpm2b": ek_pub_tpm2b,
            "ak_pub_tpm2b": ak_pub_tpm2b,
            "ak_name": ak_name,
            "secret": secret,
        }

        try:
            # make_credential returns the combined binary
            # (TPM2B_ID_OBJECT || TPM2B_ENCRYPTED_SECRET) that tpm2_activatecredential
            # reads from a single file with -i/--credential-blob.
            combined = make_credential(ek_pub_tpm2b, ak_name, secret)
        except Exception as e:
            self._send(500, {"error": f"MakeCredential failed: {e}"})
            return

        self._send(200, {
            "credential_blob_b64": base64.b64encode(combined).decode(),
        })

    # POST /v1/attestation/ak/complete
    def _complete(self):
        body = self._body()
        device_id  = body.get("device_id", "")
        secret_b64 = body.get("secret_b64", "")

        if not all([device_id, secret_b64]):
            self._send(400, {"error": "missing fields"})
            return

        entry = store.get(device_id)
        if not entry or "secret" not in entry:
            self._send(404, {"error": "no pending challenge for device"})
            return

        recovered = base64.b64decode(secret_b64)
        if not hmac.compare_digest(recovered, entry["secret"]):
            self._send(403, {"error": "secret mismatch — ActivateCredential failed"})
            return

        try:
            ak_cert_der = issue_ak_cert(device_id, entry["ak_pub_tpm2b"])
        except Exception as e:
            self._send(500, {"error": f"cert issuance failed: {e}"})
            return

        fp = cert_fingerprint(ak_cert_der)
        entry["ak_cert_der"] = ak_cert_der
        entry["ak_cert_fp_sha256"] = fp
        del entry["secret"]  # consumed

        self._send(200, {
            "ak_cert_b64": base64.b64encode(ak_cert_der).decode(),
        })

    # GET /v1/attestation/ak/status
    def _status(self):
        body = self._body()
        device_id   = body.get("device_id", "")
        ak_pub_b64  = body.get("ak_pub_b64", "")

        if not device_id:
            self._send(400, {"error": "missing device_id"})
            return

        entry = store.get(device_id)
        if not entry or "ak_cert_der" not in entry:
            self._send(404, {"error": "not registered"})
            return

        # Verify requested ak_pub matches registered one.
        if ak_pub_b64 and base64.b64decode(ak_pub_b64) != entry.get("ak_pub_tpm2b", b""):
            self._send(404, {"error": "ak_pub mismatch — not registered"})
            return

        self._send(200, {
            "ak_cert_fingerprint_sha256": entry["ak_cert_fp_sha256"],
        })

    # POST /v1/attestation/quote/verify
    def _verify_quote(self):
        """
        Verify a TPM2 quote produced by orb-tpm-quote.sh.

        Request JSON:
          { "device_id": "...",
            "nonce_hex": "<32-byte nonce as hex>",
            "quoted_b64": "<TPM2B_ATTEST base64>",
            "signature_b64": "<TPMT_SIGNATURE base64>" }

        The registered AK pub (stored as TPM2B_PUBLIC during enrollment) is
        used with tpm2_checkquote for verification.
        """
        body = self._body()
        device_id     = body.get("device_id", "")
        nonce_hex     = body.get("nonce_hex", "")
        quoted_b64    = body.get("quoted_b64", "")
        signature_b64 = body.get("signature_b64", "")

        if not all([device_id, nonce_hex, quoted_b64, signature_b64]):
            self._send(400, {"error": "missing fields: device_id, nonce_hex, quoted_b64, signature_b64"})
            return

        entry = store.get(device_id)
        if not entry or "ak_cert_der" not in entry:
            self._send(404, {"error": f"device {device_id!r} not enrolled"})
            return

        try:
            quoted_bytes    = base64.b64decode(quoted_b64)
            signature_bytes = base64.b64decode(signature_b64)
            nonce_bytes     = bytes.fromhex(nonce_hex)
        except Exception as e:
            self._send(400, {"error": f"base64/hex decode error: {e}"})
            return

        try:
            result = verify_quote(
                ak_pub_tpm2b   = entry["ak_pub_tpm2b"],
                quoted_bytes   = quoted_bytes,
                signature_bytes = signature_bytes,
                nonce_bytes    = nonce_bytes,
            )
        except Exception as e:
            self._send(500, {"error": f"quote verification error: {e}"})
            return

        if result["valid"]:
            self._send(200, result)
        else:
            self._send(403, result)


# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    p = argparse.ArgumentParser(description="fTPM attestation test backend")
    p.add_argument("--host", default="0.0.0.0")
    p.add_argument("--port", type=int, default=8080)
    args = p.parse_args()

    print(f"[server] Listening on {args.host}:{args.port}")
    print(f"[server] Set on Orb: BACKEND_URL=http://<your-PC-IP>:{args.port}")
    print(f"[server] CA cert fingerprint: {cert_fingerprint(CA_CERT.public_bytes(serialization.Encoding.DER))}")
    HTTPServer((args.host, args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
