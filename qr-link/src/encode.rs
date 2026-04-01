use data_encoding::BASE64_NOPAD;
use thiserror::Error;
use uuid::Uuid;

/// QR-code version prefix.
pub const QR_VERSION: u8 = b'4';

/// QR-code encoding error returned by [`encode_static_qr`].
#[derive(Error, Debug)]
pub enum EncodeError {
    /// The authenticated data hash must not be empty.
    #[error("authenticated data hash is empty")]
    EmptyHash,
}

/// Generates a QR-code (V4) string from `orb relay id` and `app_authenticated_data`
pub fn encode_static_qr(
    orb_relay_id: &Uuid,
    authenticated_data_hash: impl AsRef<[u8]>,
) -> Result<String, EncodeError> {
    let hash = authenticated_data_hash.as_ref();
    if hash.is_empty() {
        return Err(EncodeError::EmptyHash);
    }

    let mut payload = Vec::new();
    payload.extend_from_slice(&orb_relay_id.as_u128().to_be_bytes());
    payload.extend_from_slice(hash);

    let mut qr = String::new();
    qr.push(QR_VERSION.into());
    BASE64_NOPAD.encode_append(&payload, &mut qr);

    Ok(qr)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn empty_hash_is_rejected() {
        let id = Uuid::nil();
        assert!(matches!(
            encode_static_qr(&id, &[] as &[u8]),
            Err(EncodeError::EmptyHash)
        ));
    }

    #[test]
    fn non_empty_hash_is_accepted() {
        let id = Uuid::nil();
        assert!(encode_static_qr(&id, &[0xAB]).is_ok());
    }
}
