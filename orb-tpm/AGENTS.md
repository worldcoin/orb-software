# orb-tpm — Agent Notes

## What this crate does

`orb-tpm` is a thin TSS2 ESAPI wrapper that performs TPM2 remote attestation (quote) for the Orb.

Public API: `pub fn quote(nonce: &[u8]) -> Result<QuoteResult, Error>`

- Creates a transient SRK (ECC P-256, restricted decryption) under `Hierarchy::Owner`.
- Creates + loads a transient AIK (ECC P-256, restricted ECDSA-SHA256 signing).
- Quotes PCRs 0–7 (SHA-256 bank) with the supplied nonce.
- Flushes both transient handles.
- Returns raw TPM wire bytes: `TPM2B_ATTEST` + `TPMT_SIGNATURE`.

## File layout

```raw
orb-tpm/
  Cargo.toml            — package manifest; deps: tss-esapi, thiserror, tracing
  src/lib.rs            — entire implementation (~250 lines): srk_public(), aik_public(), quote()
  tests/integration.rs  — nonce-validation unit tests + #[ignore] simulator tests
  docker-compose.yml    — tpm-sim (swtpm) + test-runner services
  docker/
    Dockerfile.swtpm    — debian:bookworm-slim + swtpm/swtpm-tools
    Dockerfile.test     — rust:1.87-bookworm + libtss2-dev + libtss2-tcti-swtpm0
  README.md             — user-facing docs
  AGENTS.md             — this file
```

## Key implementation details

### SRK template

Uses `PublicEccParametersBuilder::new_restricted_decryption_key(AES_128_CFB, NistP256)`.
Do **not** use `PublicEccParametersBuilder::new()` for the SRK — the builder validation
requires a non-null `SymmetricDefinitionObject` for restricted-decryption keys and
`new_restricted_decryption_key()` sets the correct internal flags automatically.

### AIK template

Uses `PublicEccParametersBuilder::new()` with:
- `.with_is_signing_key(true)` — required
- `.with_restricted(true)` — required for a quoting key
- `.with_ecc_scheme(EccScheme::EcDsa(HashScheme::new(HashingAlgorithm::Sha256)))`
- `.with_key_derivation_function_scheme(KeyDerivationFunctionScheme::Null)`
- No `SymmetricDefinitionObject` — signing keys must not have a symmetric scheme

### Authorization

All TPM commands are wrapped in `ctx.execute_with_session(Some(AuthSession::Password), |ctx| { … })`.
`AuthSession::Password` maps to `ESYS_TR_PASSWORD` (the TPM's implicit password session).
This is correct when the Owner hierarchy has no auth value set (default for a fresh swtpm).

### TCTI selection

`TctiNameConf::from_environment_variable()` reads `TCTI`, then `TPM2TOOLS_TCTI`, then `TEST_TCTI`.
**Not** `TSS2_TCTI`. The Docker Compose environment sets `TCTI="swtpm:host=tpm-sim,port=2321"`.

## Running tests

### Inside Docker (recommended, works on macOS)

```sh
# From orb-software workspace root:
docker compose -f orb-tpm/docker-compose.yml run --rm --build test-runner
```

### From the host (Linux only, requires libtss2-tcti-swtpm0)

```sh
docker compose -f orb-tpm/docker-compose.yml up -d tpm-sim
TCTI="swtpm:host=127.0.0.1,port=2321" \
  cargo test -p orb-tpm --test integration -- --include-ignored --test-threads=1
docker compose -f orb-tpm/docker-compose.yml down -v
```

### Unit tests (no TPM, any OS)

```sh
cargo test -p orb-tpm
```

## Known pitfalls — always check these first

### 1. `TPM_RC_INITIALIZE` (0x0100)

**Cause:** swtpm started without `startup-clear` flag.
**Fix:** `docker-compose.yml` tpm-sim command must include `--flags not-need-init,startup-clear`.
The `not-need-init` only bypasses the control-channel handshake; `startup-clear` is what
sends `TPM2_Startup(CLEAR)` over the data channel.

### 2. `TPM_RC_OBJECT_MEMORY` (0x0902)

**Cause:** two tests running in parallel both try to load SRK + AIK into the same swtpm,
exhausting libtpms's default 3 transient-object slots (2 slots per quote() call).
**Fix:** `Dockerfile.test` CMD must include `--test-threads=1`. The swtpm is a shared
stateful resource; integration tests must run serially.

### 3. macOS build fails with "Failed to find tss2-sys library"

**Cause:** `tss-esapi-sys` has no prebuilt bindings for macOS/aarch64.
**Fix:** workspace `Cargo.toml` must enable `features = ["generate-bindings"]` for `tss-esapi`.
Also requires `clang`/`libclang` and the `tpm2-tss` headers in `PKG_CONFIG_PATH`.
Use Docker for tests instead.

### 4. Wrong env var name

`tss-esapi` reads `TCTI` / `TPM2TOOLS_TCTI` / `TEST_TCTI`. It does **not** read `TSS2_TCTI`.
Setting `TSS2_TCTI` has no effect.

### 5. `ActivateCredential` requires `PolicySecret(Endorsement)`, not `AuthSession::Password`

`AuthSession::Password` (= `ESYS_TR_PASSWORD`) works for the **Owner hierarchy** only:
SRK creation, AK creation, `Load`, `Quote`. The current `quote()` function is entirely
Owner-hierarchy and is not affected.

`ActivateCredential` must authorize the EK handle, which lives in the **Endorsement
hierarchy** with `objectAttributes.adminWithPolicy = true`. The EK well-known policy is
`PolicySecret(Endorsement, ...)`. Calling it with `AuthSession::Password` returns
`TPM_RC_POLICY_FAIL`.

The correct session setup in `tss-esapi`:

```rust
// Start a policy session
let session = ctx.start_auth_session(
    None, None, None,
    SessionType::Policy,
    SymmetricDefinition::AES_128_CFB,
    HashingAlgorithm::Sha256,
)?;

// Satisfy the EK's well-known policy
ctx.policy_secret(
    session.unwrap(),
    AuthorizationHandle::Endorsement,
    Default::default(), Default::default(), Default::default(), None,
)?;

// First session covers AK (Password), second covers EK (Policy)
ctx.execute_with_sessions(
    (Some(AuthSession::Password), Some(session.unwrap()), None),
    |ctx| ctx.activate_credential(ak_handle, ek_handle, credential_blob, encrypted_secret),
)?;
```

This is only needed when implementing `ActivateCredential` for AK enrollment.
The existing `quote()` call is unaffected.

## Integration with orb-jobs-agent

`orb-jobs-agent/src/handlers/tpm_quote.rs` wraps `orb_tpm::quote()` in a
`tokio::task::spawn_blocking` call (TSS2 ESAPI is synchronous/blocking). The handler
accepts a base64-encoded nonce via JSON args and returns a JSON object with base64-encoded
`quoted`, `signature`, and `aik_cert` fields.

## Design document

Full enrollment design with boot-time activation pseudo-code, AK re-provisioning
flow, PolicySecret explanation, and backend EK cert registry design lives in:
`docs/ftpm-enrollment-design.md`

## What is NOT in scope

- No persistent NV handle management — SRK and AIK are always transient.
- No EK certificate retrieval — `aik_cert_der` is always empty against swtpm; production
  would read NV index 0x01C0000A (ECC EK cert).
- No AIK certification flow (`ActivateCredential`).
- No PCR event log parsing.
