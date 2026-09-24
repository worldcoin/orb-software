//! PCP sealed-box encryption and legacy Hyrax commitment generation.
//!
//! This encrypts bytes for a recipient; it does not authenticate the sender or
//! authorize the recipient. The caller owns and is responsible for clearing
//! plaintext inputs. There is no plaintext-output or encryption-disable mode.

// Explicit module pins the PCP wire algorithm, independent of default re-exports.
use alkali::{
    asymmetric::seal::{curve25519xsalsa20poly1305 as sealedbox, SealError},
    AlkaliError,
};
use rand::{CryptoRng, RngCore};
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SealingError {
    #[error("recipient public key has unacceptable low order")]
    InvalidRecipient,
    #[error("sealed-box encryption failed")]
    Library(#[source] AlkaliError),
}

/// Encrypts with Curve25519/XSalsa20-Poly1305 sealed boxes and fresh ephemeral keys.
///
/// The raw recipient key is 32 bytes; ciphertext adds 48 bytes of overhead.
/// Low-order recipient keys are rejected by libsodium. This check does not
/// establish key ownership or authorization.
/// Recipient bytes are not normalized: their exact encoding participates in the
/// sealed-box nonce. Allocation or entropy-source exhaustion
/// may abort the process rather than return a recoverable error.
pub(crate) fn seal(
    plaintext: &[u8],
    recipient: &[u8; 32],
) -> Result<Vec<u8>, SealingError> {
    let mut ciphertext = vec![0; plaintext.len() + sealedbox::OVERHEAD_LENGTH];
    sealedbox::encrypt(plaintext, recipient, &mut ciphertext).map_err(|error| {
        if error == AlkaliError::SealError(SealError::PublicKeyUnacceptable) {
            SealingError::InvalidRecipient
        } else {
            SealingError::Library(error)
        }
    })?;
    Ok(ciphertext)
}

/// Generated outputs bound to the immutable data used to compute them.
///
/// Blinding factors are sensitive and must only be disclosed to the intended
/// recipient. This wrapper clears its owned blinding bytes on drop; callers own
/// the lifetime and clearing of input data and any copies of the returned bytes.
pub(crate) struct GeneratedCommitment<'a> {
    data: &'a [u8],
    commitment: Vec<u8>,
    blinding_factors: Zeroizing<Vec<u8>>,
}

impl GeneratedCommitment<'_> {
    pub(crate) fn data(&self) -> &[u8] {
        self.data
    }

    pub(crate) fn commitment(&self) -> &[u8] {
        &self.commitment
    }

    pub(crate) fn blinding_factors(&self) -> &[u8] {
        &self.blinding_factors
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommitmentError {
    #[error("could not generate Hyrax blinding seed")]
    Randomness(#[source] rand::Error),
}

/// Generates using the same pinned implementation and fresh 32-byte seed as core.
/// Inputs are padded to a power of two; inputs of 256 bytes or fewer preserve
/// the legacy empty-commitment behavior. This does not verify imported commitments.
///
/// The caller supplies a cryptographically secure random source. Entropy failure
/// returns no result. Computation is synchronous and may be expensive; async
/// consumers must run it on their blocking worker. The local seed is cleared on
/// drop. Upstream takes the seed by value and does not clear that copy or all
/// internal blinding values; this is not comprehensive memory erasure.
pub(crate) fn generate_commitment<'a>(
    data: &'a [u8],
    rng: &mut (impl RngCore + CryptoRng),
) -> Result<GeneratedCommitment<'a>, CommitmentError> {
    let mut seed = Zeroizing::new([0; 32]);
    rng.try_fill_bytes(seed.as_mut())
        .map_err(CommitmentError::Randomness)?;
    let output =
        hyrax::iriscode_commit::compute_commitments_binary_outputs(data, *seed);
    Ok(GeneratedCommitment {
        data,
        commitment: output.commitment_serialized,
        blinding_factors: Zeroizing::new(output.blinding_factors_serialized),
    })
}

#[cfg(test)]
#[path = "../tests/unit/crypto.rs"]
mod tests;
