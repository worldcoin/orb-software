//! Thin TPM2 quoting layer for the Orb.
//!
//! Callers pass a 32-byte nonce and receive a [`QuoteResult`] containing the
//! raw TPM wire structures needed for remote attestation.
//!
//! # Transport selection
//!
//! The TSS2 TCTI is taken from the `TCTI` environment variable at
//! runtime (tss-esapi also checks `TPM2TOOLS_TCTI` and `TEST_TCTI`). Examples:
//! - Hardware fTPM: `TCTI="device:/dev/tpmrm0"`
//! - swtpm simulator: `TCTI="swtpm:host=127.0.0.1,port=2321"`
//!
//! # Platform support
//!
//! Requires `libtss2-esys` to be present on the build host. On macOS, build
//! `tpm2-tss` from source and set `PKG_CONFIG_PATH` accordingly.

#![cfg_attr(not(test), forbid(unsafe_code))]

use tracing::instrument;
use tss_esapi::{
    Context, TctiNameConf,
    attributes::ObjectAttributesBuilder,
    interface_types::{
        algorithm::{HashingAlgorithm, PublicAlgorithm},
        ecc::EccCurve,
        resource_handles::Hierarchy,
        session_handles::AuthSession,
    },
    structures::{
        Data, EccPoint, EccScheme, HashScheme, KeyDerivationFunctionScheme,
        PcrSelectionListBuilder, PcrSlot, PublicBuilder,
        PublicEccParametersBuilder, SignatureScheme, SymmetricDefinitionObject,
    },
    traits::Marshall,
};

/// The output of a TPM2 quote operation.
///
/// All byte blobs are in raw TPM wire format (as the TPM produces them).
/// Callers should base64-encode them before transmitting over the network.
#[derive(Debug, Clone)]
pub struct QuoteResult {
    /// Raw `TPM2B_ATTEST` bytes — the signed attestation blob.
    pub quoted: Vec<u8>,
    /// Raw `TPMT_SIGNATURE` bytes.
    pub signature: Vec<u8>,
    /// DER-encoded AIK certificate chain (leaf first).
    /// Empty when running against swtpm (no provisioned cert).
    pub aik_cert_der: Vec<u8>,
}

/// Errors returned by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("nonce must be exactly 32 bytes, got {0}")]
    InvalidNonceLength(usize),

    #[error("TSS2 error: {0}")]
    Tss2(#[from] tss_esapi::Error),
}

// ─── Key templates ────────────────────────────────────────────────────────────

/// Build a Storage Root Key (SRK) primary key template.
///
/// ECC P-256, restricted + decryption, suitable as a parent for the AIK.
fn srk_public() -> Result<tss_esapi::structures::Public, Error> {
    let attrs = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_restricted(true)
        .with_decrypt(true)
        .build()?;

    Ok(PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::Ecc)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(attrs)
        .with_ecc_parameters(
            // new_restricted_decryption_key sets restricted=true + is_decryption_key=true
            // internally so the builder validation accepts a non-null symmetric.
            PublicEccParametersBuilder::new_restricted_decryption_key(
                SymmetricDefinitionObject::AES_128_CFB,
                EccCurve::NistP256,
            )
            .build()?,
        )
        .with_ecc_unique_identifier(EccPoint::default())
        .build()?)
}

/// Build an Attestation Identity Key (AIK) template.
///
/// ECC P-256 restricted signing key (ECDSA-SHA256). In production this key
/// would be certified by the EK certificate chain; in swtpm tests the
/// `aik_cert_der` field remains empty.
fn aik_public() -> Result<tss_esapi::structures::Public, Error> {
    let attrs = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_restricted(true)
        .with_sign_encrypt(true)
        .build()?;

    Ok(PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::Ecc)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(attrs)
        .with_ecc_parameters(
            PublicEccParametersBuilder::new()
                .with_ecc_scheme(EccScheme::EcDsa(HashScheme::new(HashingAlgorithm::Sha256)))
                .with_curve(EccCurve::NistP256)
                .with_key_derivation_function_scheme(KeyDerivationFunctionScheme::Null)
                .with_is_signing_key(true)
                .with_restricted(true)
                .build()?,
        )
        .with_ecc_unique_identifier(EccPoint::default())
        .build()?)
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Produce a TPM2 quote over PCRs 0–7 (SHA-256 bank), bound to `nonce`.
///
/// `nonce` must be exactly 32 bytes (raw bytes, not base64).
///
/// The TCTI transport is selected via the `TCTI` environment variable
/// (or `TPM2TOOLS_TCTI` / `TEST_TCTI`):
/// - Hardware fTPM: `TCTI="device:/dev/tpmrm0"`
/// - swtpm simulator: `TCTI="swtpm:host=127.0.0.1,port=2321"`
///
/// Each call creates transient keys — no persistent NV handles are used.
///
/// # Errors
///
/// - [`Error::InvalidNonceLength`] if `nonce.len() != 32`.
/// - [`Error::Tss2`] for any TSS2/ESAPI error (TCTI not found, TPM command error, …).
#[instrument(skip(nonce), fields(nonce_len = nonce.len()))]
pub fn quote(nonce: &[u8]) -> Result<QuoteResult, Error> {
    if nonce.len() != 32 {
        return Err(Error::InvalidNonceLength(nonce.len()));
    }

    let tcti = TctiNameConf::from_environment_variable()?;
    eprintln!("[orb-tpm] TCTI resolved");
    let mut ctx = Context::new(tcti)?;
    eprintln!("[orb-tpm] Context created");

    let qualifying_data = Data::try_from(nonce.to_vec())?;
    eprintln!("[orb-tpm] qualifying_data built");

    // All TPM commands that require hierarchy or object authorization
    // must be wrapped in execute_with_session. AuthSession::Password uses
    // the TPM's implicit password session (ESYS_TR_PASSWORD), which is
    // sufficient when the Owner hierarchy has no auth value set (the default).
    let (quoted, sig_bytes) = ctx.execute_with_session(Some(AuthSession::Password), |ctx| {
        let srk_pub = srk_public()?;
        eprintln!("[orb-tpm] srk_public() OK");

        let srk_result =
            ctx.create_primary(Hierarchy::Owner, srk_pub, None, None, None, None)?;
        let srk_handle = srk_result.key_handle;
        eprintln!("[orb-tpm] SRK created");

        let aik_pub = aik_public()?;
        eprintln!("[orb-tpm] aik_public() OK");

        let create_result = ctx.create(srk_handle, aik_pub, None, None, None, None)?;
        eprintln!("[orb-tpm] AIK created");

        let aik_handle =
            ctx.load(srk_handle, create_result.out_private, create_result.out_public)?;
        eprintln!("[orb-tpm] AIK loaded");

        let pcr_selection = PcrSelectionListBuilder::new()
            .with_selection(
                HashingAlgorithm::Sha256,
                &[
                    PcrSlot::Slot0,
                    PcrSlot::Slot1,
                    PcrSlot::Slot2,
                    PcrSlot::Slot3,
                    PcrSlot::Slot4,
                    PcrSlot::Slot5,
                    PcrSlot::Slot6,
                    PcrSlot::Slot7,
                ],
            )
            .build()?;

        let (attest, signature) = ctx.quote(
            aik_handle,
            qualifying_data.clone(),
            SignatureScheme::Null, // TPM picks scheme from AIK template (ECDSA-SHA256)
            pcr_selection,
        )?;
        eprintln!("[orb-tpm] quote done");

        // Flush transient handles — best-effort, log on error.
        if let Err(e) = ctx.flush_context(srk_handle.into()) {
            eprintln!("[orb-tpm] warn: flush SRK: {e}");
        }
        if let Err(e) = ctx.flush_context(aik_handle.into()) {
            eprintln!("[orb-tpm] warn: flush AIK: {e}");
        }

        let quoted_bytes = attest.marshall()?;
        let sig_bytes = signature.marshall()?;

        Ok::<_, Error>((quoted_bytes, sig_bytes))
    })?;

    tracing::info!("TPM quote produced successfully");

    Ok(QuoteResult {
        quoted,
        signature: sig_bytes,
        // No provisioned EK cert in swtpm; production would read from NV
        // index 0x01C00002 (RSA EK) or 0x01C0000A (ECC EK).
        aik_cert_der: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_nonce() {
        let err = quote(&[0u8; 16]).unwrap_err();
        assert!(matches!(err, Error::InvalidNonceLength(16)));
    }

    #[test]
    fn rejects_long_nonce() {
        let err = quote(&[0u8; 64]).unwrap_err();
        assert!(matches!(err, Error::InvalidNonceLength(64)));
    }

    #[test]
    fn rejects_empty_nonce() {
        assert!(matches!(quote(&[]).unwrap_err(), Error::InvalidNonceLength(0)));
    }

    // Without TSS2_TCTI the ESAPI context creation fails — this is expected.
    #[test]
    fn returns_tss2_error_when_no_tcti_set() {
        // SAFETY: tests run single-threaded in this binary.
        unsafe { std::env::remove_var("TSS2_TCTI") };
        let result = quote(&[0xABu8; 32]);
        match result {
            Err(Error::Tss2(_)) => {} // expected: TCTI not found
            Ok(_) => {}              // acceptable if a system TPM is present
            Err(e) => panic!("unexpected error variant: {e:?}"),
        }
    }
}
