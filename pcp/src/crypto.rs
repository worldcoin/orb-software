//! PCP sealed-box encryption and legacy Hyrax commitment generation.
//!
//! This encrypts bytes for a recipient; it does not authenticate the sender or
//! authorize the recipient. The caller owns and is responsible for clearing
//! plaintext inputs. There is no plaintext-output or encryption-disable mode.

use rand::{CryptoRng, RngCore};
use sodiumoxide::crypto::{box_, scalarmult::curve25519, sealedbox};
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SealingError {
    #[error("could not initialize libsodium")]
    Initialization,
    #[error("recipient public key has unacceptable low order")]
    InvalidRecipient,
    #[error("plaintext is too large for a sealed box")]
    MessageTooLong,
}

/// Encrypts with Curve25519/XSalsa20-Poly1305 sealed boxes and fresh ephemeral keys.
///
/// The raw recipient key is 32 bytes; ciphertext adds 48 bytes of overhead.
/// Low-order recipient keys are rejected before calling sodiumoxide's infallible
/// sealing API. This check does not establish key ownership or authorization.
/// Recipient bytes are not normalized: their exact encoding participates in the
/// sealed-box nonce. As in sodiumoxide, allocation or entropy-source exhaustion
/// may abort the process rather than return a recoverable error.
pub(crate) fn seal(
    plaintext: &[u8],
    recipient: &[u8; 32],
) -> Result<Vec<u8>, SealingError> {
    ciphertext_length(plaintext.len())?;
    sodiumoxide::init().map_err(|()| SealingError::Initialization)?;

    // sodiumoxide::sealedbox::seal discards the native failure status. Check its
    // low-order rejection with the same fallible X25519 primitive first. Sodium
    // clamps this public probe scalar to a nonzero value; it is never an
    // encryption key, and the multiplication result is discarded.
    let _ = curve25519::scalarmult(
        &curve25519::Scalar([0; 32]),
        &curve25519::GroupElement(*recipient),
    )
    .map_err(|()| SealingError::InvalidRecipient)?;

    Ok(sealedbox::seal(plaintext, &box_::PublicKey(*recipient)))
}

fn ciphertext_length(plaintext_length: usize) -> Result<usize, SealingError> {
    plaintext_length
        .checked_add(sealedbox::SEALBYTES)
        .filter(|&length| length <= isize::MAX as usize)
        .ok_or(SealingError::MessageTooLong)
}

#[cfg(test)]
mod length_tests {
    use super::*;

    #[test]
    fn ciphertext_length_rejects_overflow_and_unrepresentable_allocations() {
        assert_eq!(ciphertext_length(0), Ok(48));
        assert_eq!(
            ciphertext_length(usize::MAX),
            Err(SealingError::MessageTooLong)
        );
        assert_eq!(
            ciphertext_length(isize::MAX as usize),
            Err(SealingError::MessageTooLong)
        );
        assert_eq!(
            ciphertext_length(isize::MAX as usize - 48),
            Ok(isize::MAX as usize)
        );
    }
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
/// drop, but the upstream implementation does not clear all internal copies.
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
