use data_encoding::BASE64_NOPAD;
use thiserror::Error;
use uuid::Uuid;

/// QR-code decoding error returned by [`decode_qr_with_version`].
#[derive(Error, Debug)]
pub enum DecodeError {
    /// Format version is unsupported.
    #[error("unsupported qr-code version")]
    UnsupportedVersion,
    /// QR-code is malformed.
    #[error("qr-code is malformed")]
    Malformed,
    /// Error decoding BASE64.
    #[error("invalid base64")]
    Base64,
}

/// Parses `user_id` and `user_data_hash` from a QR-code string and return the version
/// different logic on the orb can be done based on the version returned
pub fn decode_qr_with_version(qr: &str) -> Result<(u8, Uuid, Vec<u8>), DecodeError> {
    let Some(version) = qr.bytes().next() else {
        return Err(DecodeError::Malformed);
    };
    match version {
        b'4' => {
            let (orb_relay_id, app_authenticated_data_hash) = decode_v4(qr)?;
            Ok((4, orb_relay_id, app_authenticated_data_hash))
        }
        _ => Err(DecodeError::UnsupportedVersion),
    }
}

// the `decode_v4` method is specifically to decode static sessions where the id is the `orb_relay_id` and the hash is the hash from `AppAuthenticatedData`
fn decode_v4(qr: &str) -> Result<(Uuid, Vec<u8>), DecodeError> {
    let Ok(payload) = BASE64_NOPAD.decode(&qr.as_bytes()[1..]) else {
        return Err(DecodeError::Base64);
    };
    let Some(orb_relay_id) = payload.get(0..16) else {
        return Err(DecodeError::Malformed);
    };
    let app_authenticated_data_hash = payload
        .get(16..)
        .filter(|h| !h.is_empty())
        .ok_or(DecodeError::Malformed)?;
    let orb_relay_id = u128::from_be_bytes(orb_relay_id.try_into().unwrap());
    let orb_relay_id = Uuid::from_u128(orb_relay_id);

    Ok((orb_relay_id, app_authenticated_data_hash.to_vec()))
}

#[cfg(test)]
mod tests {
    use data_encoding::BASE64_NOPAD;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn uuid_only_payload_is_rejected() {
        // 16 bytes = UUID with no hash bytes
        let uuid = Uuid::nil();
        let payload = uuid.as_u128().to_be_bytes();
        let mut qr = String::from("4");
        BASE64_NOPAD.encode_append(&payload, &mut qr);

        assert!(matches!(
            decode_qr_with_version(&qr),
            Err(DecodeError::Malformed)
        ));
    }

    #[test]
    fn uuid_with_hash_bytes_is_accepted() {
        let uuid = Uuid::nil();
        let mut payload = uuid.as_u128().to_be_bytes().to_vec();
        payload.push(0xAB);
        let mut qr = String::from("4");
        BASE64_NOPAD.encode_append(&payload, &mut qr);

        let (version, id, hash) = decode_qr_with_version(&qr).unwrap();
        assert_eq!(version, 4);
        assert_eq!(id, uuid);
        assert_eq!(hash, vec![0xAB]);
    }

    #[test]
    fn too_short_payload_is_rejected() {
        // Less than 16 bytes
        let mut qr = String::from("4");
        BASE64_NOPAD.encode_append(&[0u8; 8], &mut qr);

        assert!(matches!(
            decode_qr_with_version(&qr),
            Err(DecodeError::Malformed)
        ));
    }

    #[test]
    fn empty_qr_is_rejected() {
        assert!(matches!(
            decode_qr_with_version(""),
            Err(DecodeError::Malformed)
        ));
    }

    #[test]
    fn unsupported_version_is_rejected() {
        assert!(matches!(
            decode_qr_with_version("3AAAA"),
            Err(DecodeError::UnsupportedVersion)
        ));
    }
}
