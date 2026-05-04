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
//! after starting the simulator:
//!
//! ```sh
//! # In the orb-tpm directory:
//! docker compose up -d tpm-sim
//!
//! # Wait for healthy, then:
//!   TCTI="swtpm:host=127.0.0.1,port=2321" \
//!   cargo test -p orb-tpm --test integration -- --include-ignored
//!
//! docker compose down -v
//! ```
//!
//! The `TSS2_TCTI` environment variable selects the TCTI transport.  The
//! simulator above speaks the swtpm TCP protocol, so the value is
//! `swtpm:host=127.0.0.1,port=2321` (requires `libtss2-tcti-swtpm.so`).
//! Against a hardware TPM the typical value is `device:/dev/tpmrm0`.

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

// ── Simulator / hardware tests (require TSS2_TCTI) ──────────────────────
//
// Start the simulator before running:
//   docker compose up -d tpm-sim
//   TSS2_TCTI="swtpm:host=127.0.0.1,port=2321" \
//     cargo test -p orb-tpm --test integration -- --include-ignored

/// Verify that a real TPM (or simulator) produces a non-empty quote.
///
/// Requires `TSS2_TCTI` to point at a running TPM or simulator.
/// See module-level docs for how to start the simulator.
#[test]
#[ignore = "requires a running TPM simulator (see module docs)"]
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
/// Requires `TSS2_TCTI` to point at a running TPM or simulator.
#[test]
#[ignore = "requires a running TPM simulator (see module docs)"]
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
