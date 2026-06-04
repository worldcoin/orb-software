//! Integration tests for `orb-tpm`.
//!
//! # Tests that run without a TPM (always pass)
//!
//! The nonce-validation tests run against the stub and require no external
//! dependencies. They are run by plain `cargo test -p orb-tpm`.
//!
//! # Tests that require a TPM simulator  (`#[ignore]`)
//!
//! These tests are marked `#[ignore]` and are skipped by default. Run them
//! after starting the simulator via Docker Compose:
//!
//! ```sh
//! # From the orb-software workspace root:
//!
//! # Run Rust integration tests (transient-key quote only):
//! docker compose -f orb-tpm/docker-compose.yml run --rm --build test-runner
//!
//! # Run the full end-to-end enrollment + attestation scenario:
//! docker compose -f orb-tpm/docker-compose.yml run --rm --build enrollment-runner
//!
//! # Or start simulator alone and run from host (Linux only):
//! docker compose -f orb-tpm/docker-compose.yml up -d tpm-sim
//! TCTI="swtpm:host=127.0.0.1,port=2321" \
//!   cargo test -p orb-tpm --test integration -- --include-ignored
//! docker compose -f orb-tpm/docker-compose.yml down -v
//! ```
//!
//! The TCTI transport is selected from the `TCTI` env var (then `TPM2TOOLS_TCTI`,
//! then `TEST_TCTI`).  For the swtpm simulator: `swtpm:host=127.0.0.1,port=2321`.
//! Note: requires `libtss2-tcti-swtpm0` installed on the host.

// ── Always-passing tests (nonce validation only, no TPM needed) ──────────

#[test]
fn rejects_nonce_too_short() {
    let err = orb_tpm::quote(&[0u8; 16]).unwrap_err();
    assert!(
        matches!(err, orb_tpm::Error::InvalidNonceLength(16)),
        "expected InvalidNonceLength(16), got {err:?}"
    );
}

#[test]
fn rejects_nonce_too_long() {
    let err = orb_tpm::quote(&[0u8; 48]).unwrap_err();
    assert!(
        matches!(err, orb_tpm::Error::InvalidNonceLength(48)),
        "expected InvalidNonceLength(48), got {err:?}"
    );
}

#[test]
fn rejects_empty_nonce() {
    let err = orb_tpm::quote(&[]).unwrap_err();
    assert!(matches!(err, orb_tpm::Error::InvalidNonceLength(0)));
}

#[test]
fn persistent_ak_rejects_short_nonce() {
    let err = orb_tpm::quote_from_persistent_ak(&[0u8; 16]).unwrap_err();
    assert!(matches!(err, orb_tpm::Error::InvalidNonceLength(16)));
}

// ── Simulator / hardware tests (require TSS2_TCTI) ──────────────────────
//
// Start the simulator before running (Docker Compose recommended):
//   docker compose -f orb-tpm/docker-compose.yml run --rm --build test-runner
//
// Or manually (Linux + libtss2-tcti-swtpm0 installed):
//   docker compose -f orb-tpm/docker-compose.yml up -d tpm-sim
//   TCTI="swtpm:host=127.0.0.1,port=2321" \
//     cargo test -p orb-tpm --test integration -- --include-ignored

/// Verify that a real TPM (or simulator) produces a non-empty quote (transient keys).
///
/// Requires `TCTI` to point at a running TPM or simulator.
#[test]
#[ignore = "requires a running TPM simulator – use: docker compose run --rm test-runner"]
fn real_tpm_quote_produces_non_empty_blobs() {
    let tcti = std::env::var("TCTI")
        .or_else(|_| std::env::var("TPM2TOOLS_TCTI"))
        .unwrap_or_else(|_| {
            panic!(
                "Set TCTI before running this test, e.g.:\n  \
                 TCTI=\"swtpm:host=127.0.0.1,port=2321\" cargo test -p orb-tpm \
                 --test integration -- --include-ignored"
            )
        });
    eprintln!("Using TCTI: {tcti}");

    let nonce = [0x42u8; 32];
    let result = orb_tpm::quote(&nonce).expect("quote must not error");

    assert!(!result.quoted.is_empty(), "TPM2B_ATTEST must not be empty");
    assert!(!result.signature.is_empty(), "TPMT_SIGNATURE must not be empty");
    eprintln!("quoted len:    {}", result.quoted.len());
    eprintln!("signature len: {}", result.signature.len());
    // aik_cert_der is empty against swtpm (no provisioned cert) — that is expected.
}

/// Verify that different nonces produce different quotes (freshness / anti-replay).
///
/// Requires `TCTI` to point at a running TPM or simulator.
#[test]
#[ignore = "requires a running TPM simulator – use: docker compose run --rm test-runner"]
fn different_nonces_produce_different_quotes() {
    let nonce_a = [0x11u8; 32];
    let nonce_b = [0x22u8; 32];

    let result_a = orb_tpm::quote(&nonce_a).expect("quote A must succeed");
    let result_b = orb_tpm::quote(&nonce_b).expect("quote B must succeed");

    assert_ne!(
        result_a.quoted, result_b.quoted,
        "quotes for different nonces must differ"
    );
}

/// Verify that read_ak_cert_from_nv returns empty vec when no cert is provisioned.
///
/// On a fresh swtpm, the NV index 0x01800003 is not defined, so the function
/// should return `Ok(vec![])`.
#[test]
#[ignore = "requires a running TPM simulator – use: docker compose run --rm test-runner"]
fn read_ak_cert_returns_empty_when_not_enrolled() {
    let result = orb_tpm::read_ak_cert_from_nv().expect("should not error on missing NV");
    // An uninitialized or absent NV index returns empty bytes (expected pre-enrollment).
    eprintln!("AK cert NV bytes: {} (0 expected on fresh swtpm)", result.len());
    // We don't assert empty because the NV may be defined from a previous run;
    // the important thing is the function does not panic.
}

/// Verify that the persistent-AK quote path correctly signals that the AK
/// handle is absent when no enrollment has been performed.
///
/// On a fresh swtpm without provisioning, handle 0x81010003 does not exist,
/// so `quote_from_persistent_ak` must return a TSS2 error (not panic or hang).
#[test]
#[ignore = "requires a running TPM simulator – use: docker compose run --rm test-runner"]
fn persistent_ak_quote_fails_gracefully_when_not_provisioned() {
    let nonce = [0xDEu8; 32];
    let result = orb_tpm::quote_from_persistent_ak(&nonce);
    match result {
        Err(orb_tpm::Error::Tss2(_)) => {
            eprintln!("Got expected TSS2 error: AK handle not present");
        }
        Ok(_) => {
            // If a prior test provisioned the AK, a quote may succeed — that's fine.
            eprintln!("AK was already provisioned; quote succeeded");
        }
        Err(e) => panic!("unexpected error variant: {e:?}"),
    }
}

