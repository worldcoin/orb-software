//! Handles validating signatures

use base64::Engine as _;
use ed25519_dalek::VerifyingKey;
use tracing::{error, info};

#[derive(Debug, thiserror::Error)]
pub enum ManifestVerificationError {
    #[error("Base64 Decode failed: {0}")]
    Base64Decode(#[from] base64::DecodeError),
    #[error("Invalid manifest signature: {0}")]
    InvalidSignature(#[from] ed25519_dalek::SignatureError),
}

/// Verifies that the base64 encoded signature (which signs `message`) is valid, using
/// the provided pubkey.
pub(crate) fn verify_signature(
    pubkey: &VerifyingKey,
    base64_encoded_signature: &str,
    message: &[u8],
) -> Result<(), ManifestVerificationError> {
    // Retrieve signature from base64
    let signature_bytes =
        base64::prelude::BASE64_STANDARD.decode(base64_encoded_signature)?;
    let signature = ed25519_dalek::Signature::from_slice(&signature_bytes)?;
    info!(?signature, "got manifest signature");

    pubkey
        .verify_strict(message, &signature)
        .inspect_err(|err| {
            error!(
                ?signature,
                contents=?message,
                ?err,
                "Signature was invalid"
            )
        })?;

    Ok(())
}
