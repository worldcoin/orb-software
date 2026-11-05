// Explicit module pins the PCP wire algorithm, independent of default re-exports.
use alkali::{
    asymmetric::seal::{curve25519xsalsa20poly1305 as sealedbox, SealError},
    AlkaliError,
};

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
/// libsodium rejects low-order recipient keys. The exact recipient bytes enter
/// the sealed-box nonce. Allocation or entropy-source exhaustion may abort the
/// process instead of returning an error.
pub fn seal(plaintext: &[u8], recipient: &[u8; 32]) -> Result<Vec<u8>, SealingError> {
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
