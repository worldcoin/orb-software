# orb-tpm

Thin TPM2 quoting layer for the Orb. Provides a single public function, `quote(nonce: &[u8])`,
that creates a transient Storage Root Key (SRK) and Attestation Identity Key (AIK), quotes
PCRs 0-7 (SHA-256 bank), and returns the raw TPM wire structures needed for remote attestation.

## Public API

```rust
pub fn quote(nonce: &[u8]) -> Result<QuoteResult, Error>
```

- `nonce` must be exactly 32 bytes (raw bytes, not base64).
- Returns [`QuoteResult`] with:
  - `quoted` — raw `TPM2B_ATTEST` bytes (the signed attestation blob)
  - `signature` — raw `TPMT_SIGNATURE` bytes
  - `aik_cert_der` — DER-encoded AIK certificate chain (leaf first); empty against swtpm

### Key design decisions

- **Transient keys only** — no persistent NV handles are used. Every `quote()` call creates
  fresh SRK and AIK keys, then flushes them.
- **ECC P-256 throughout** — SRK is a restricted-decryption key; AIK is a restricted
  ECDSA-SHA256 signing key. Matches the TPM2 reference implementation's recommended AK template.
- **`AuthSession::Password`** — uses the TPM's implicit password session (`ESYS_TR_PASSWORD`),
  correct when the Owner hierarchy has no auth value (default).

## Transport selection

The TSS2 TCTI is read from the environment at runtime. The `tss-esapi` crate checks these
variables in order: `TCTI`, `TPM2TOOLS_TCTI`, `TEST_TCTI`.

| Environment | Value |
|---|---|
| Hardware fTPM on Orb | `TCTI="device:/dev/tpmrm0"` |
| swtpm simulator (local host) | `TCTI="swtpm:host=127.0.0.1,port=2321"` |
| swtpm inside Docker Compose | `TCTI="swtpm:host=tpm-sim,port=2321"` |

## Platform support

On Linux, install from apt:

```sh
apt-get install libtss2-dev libtss2-tcti-swtpm0
```

On macOS there are no prebuilt packages. The `generate-bindings` Cargo feature must be enabled
and `tpm2-tss` must be built from source with `PKG_CONFIG_PATH` set. For day-to-day development
on macOS, use Docker (see below).

## Running integration tests

### Fully inside Docker (works on macOS, Linux, CI)

```sh
# From the orb-software workspace root:
docker compose -f orb-tpm/docker-compose.yml run --rm --build test-runner
```

The Docker Compose setup starts an swtpm simulator (`tpm-sim`) and a Debian test-runner image
that has `libtss2-dev` installed from apt. The test-runner waits for the simulator to become
healthy, then runs the integration tests serially.

### From the host (Linux only)

```sh
docker compose -f orb-tpm/docker-compose.yml up -d tpm-sim

TCTI="swtpm:host=127.0.0.1,port=2321" \
  cargo test -p orb-tpm --test integration -- --include-ignored --test-threads=1

docker compose -f orb-tpm/docker-compose.yml down -v
```

### Unit tests (no TPM required)

```sh
cargo test -p orb-tpm
```

These validate nonce length checking only and run on any host.

## Pitfalls

### `TPM_RC_INITIALIZE` (0x0100) — swtpm missing `startup-clear`

swtpm must be launched with `--flags not-need-init,startup-clear`. The `startup-clear` flag
causes swtpm to automatically issue `TPM2_Startup(CLEAR)` before accepting commands. Without
it every TPM command returns 0x0100. See the `docker-compose.yml` `tpm-sim` service.

### `TPM_RC_OBJECT_MEMORY` (0x0902) — parallel tests exhausting transient slots

libtpms defaults to 3 transient-object slots. The `quote()` function uses 2 per call (SRK +
loaded AIK). Two tests running in parallel against the same simulator need 4 slots -> exhaustion.

**Fix:** always run simulator-backed tests with `--test-threads=1`. The `Dockerfile.test` CMD
already does this.

### `generate-bindings` feature on macOS

`tss-esapi-sys` ships prebuilt FFI bindings for Linux/aarch64 only. On macOS you must enable
the `generate-bindings` feature in the workspace `Cargo.toml`:

```toml
tss-esapi = { version = "7", features = ["generate-bindings"] }
```
