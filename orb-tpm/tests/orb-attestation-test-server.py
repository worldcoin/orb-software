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


# ── TPM2B_PUBLIC parser (ECC P-256 only) ─────────────────────────────────────

def parse_tpm2b_public_ecc(data: bytes):
    """Return (x_bytes, y_bytes) from a TPM2B_PUBLIC ECC wire blob."""
    offset = 2  # skip outer size field
    # TPMT_PUBLIC header: type(2) nameAlg(2) objectAttributes(4)
    offset += 2 + 2 + 4
    # authPolicy: TPM2B — skip
    auth_size = struct.unpack_from(">H", data, offset)[0]
    offset += 2 + auth_size
    # TPMU_PUBLIC_PARMS for ECC:
    #   symmetric: algId(2) keyBits(2) mode(2)  [for AES-128-CFB]
    #   scheme:    algId(2)                     [TPM_ALG_NULL]
    #   curveID:   (2)
    #   kdf:       algId(2)                     [TPM_ALG_NULL]
    offset += 2 + 2 + 2 + 2 + 2 + 2  # symmetric + scheme + curveID + kdf
    # TPMU_PUBLIC_ID: TPM2B_ECC_POINT = x(TPM2B) | y(TPM2B)
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


# ── MakeCredential — delegates to tpm2_makecredential --tcti none ────────────
#
# tpm2_makecredential is a pure-crypto operation: it does not talk to a TPM.
# --tcti none tells it to skip device lookup entirely.
# This is the canonical implementation; rolling our own KDFe/KDFa/AES-CFB is
# error-prone and bypasses years of interop testing in tpm2-software.

def make_credential(ek_pub_tpm2b: bytes, ak_name: bytes, secret: bytes):
    """
    Delegate MakeCredential to tpm2_makecredential (tpm2-tools, --tcti none).
    Returns (credential_blob: bytes, encrypted_secret: bytes).
    """
    with tempfile.TemporaryDirectory() as tmp:
        ek_pub_file      = os.path.join(tmp, "ek.pub")
        ak_name_file     = os.path.join(tmp, "ak.name")
        secret_file      = os.path.join(tmp, "secret.bin")
        cred_blob_file   = os.path.join(tmp, "cred.blob")
        enc_secret_file  = os.path.join(tmp, "enc.secret")

        with open(ek_pub_file,  "wb") as f: f.write(ek_pub_tpm2b)
        with open(ak_name_file, "wb") as f: f.write(ak_name)
        with open(secret_file,  "wb") as f: f.write(secret)

        subprocess.run(
            [
                "tpm2_makecredential",
                "--tcti",             "none",
                "--encryption-key",   ek_pub_file,
                "--name",             ak_name_file,
                "--secret",           secret_file,
                "--credential-blob",  cred_blob_file,
                "--encrypted-secret", enc_secret_file,
            ],
            check=True,
            capture_output=True,
        )

        with open(cred_blob_file,  "rb") as f: cred_blob   = f.read()
        with open(enc_secret_file, "rb") as f: enc_secret  = f.read()

    return cred_blob, enc_secret


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
        else:
            self._send(404, {"error": "not found"})

    def do_GET(self):
        if self.path == "/v1/attestation/ak/status":
            self._status()
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
            cred_blob, enc_secret = make_credential(ek_pub_tpm2b, ak_name, secret)
        except Exception as e:
            self._send(500, {"error": f"MakeCredential failed: {e}"})
            return

        self._send(200, {
            "credential_blob_b64":   base64.b64encode(cred_blob).decode(),
            "encrypted_secret_b64":  base64.b64encode(enc_secret).decode(),
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
