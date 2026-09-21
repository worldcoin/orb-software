//! PCP sealed-box encryption using the existing sodiumoxide implementation.
//!
//! This encrypts bytes for a recipient; it does not authenticate the sender or
//! authorize the recipient. The caller owns and is responsible for clearing
//! plaintext inputs. There is no plaintext-output or encryption-disable mode.

use sodiumoxide::crypto::{box_, scalarmult::curve25519, sealedbox};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
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
pub fn seal(plaintext: &[u8], recipient: &[u8; 32]) -> Result<Vec<u8>, Error> {
    ciphertext_length(plaintext.len())?;
    sodiumoxide::init().map_err(|()| Error::Initialization)?;

    // sodiumoxide::sealedbox::seal discards the native failure status. Check its
    // low-order rejection with the same fallible X25519 primitive first. Sodium
    // clamps this public probe scalar to a nonzero value; it is never an
    // encryption key, and the multiplication result is discarded.
    let _ = curve25519::scalarmult(
        &curve25519::Scalar([0; 32]),
        &curve25519::GroupElement(*recipient),
    )
    .map_err(|()| Error::InvalidRecipient)?;

    Ok(sealedbox::seal(plaintext, &box_::PublicKey(*recipient)))
}

fn ciphertext_length(plaintext_length: usize) -> Result<usize, Error> {
    plaintext_length
        .checked_add(sealedbox::SEALBYTES)
        .filter(|&length| length <= isize::MAX as usize)
        .ok_or(Error::MessageTooLong)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciphertext_length_rejects_overflow_and_unrepresentable_allocations() {
        assert_eq!(ciphertext_length(0), Ok(48));
        assert_eq!(ciphertext_length(usize::MAX), Err(Error::MessageTooLong));
        assert_eq!(
            ciphertext_length(isize::MAX as usize),
            Err(Error::MessageTooLong)
        );
        assert_eq!(
            ciphertext_length(isize::MAX as usize - 48),
            Ok(isize::MAX as usize)
        );
    }
}
