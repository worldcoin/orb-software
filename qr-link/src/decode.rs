use data_encoding::BASE64_NOPAD;
use thiserror::Error;
use uuid::Uuid;

/// QR-code decoding error returned by [`decode_qr`] and [`decode_qr_with_version`].
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

/// Parses `session_id` and `user_data_hash` from a QR-code string.
/// This decode version does not support v4 since no v4 should be used with this method
/// All orbs should be updated to support v4 since it requires specific logic
pub fn decode_qr(qr: &str) -> Result<(Uuid, Vec<u8>), DecodeError> {
    let Some(version) = qr.bytes().next() else {
        return Err(DecodeError::Malformed);
    };
    match version {
        b'3' => decode_v3(qr),
        _ => Err(DecodeError::UnsupportedVersion),
    }
}

/// Parses `user_id` and `user_data_hash` from a QR-code string and return the version
/// different logic on the orb can be done based on the version returned
pub fn decode_qr_with_version(qr: &str) -> Result<(u8, Uuid, Vec<u8>), DecodeError> {
    let Some(version) = qr.bytes().next() else {
        return Err(DecodeError::Malformed);
    };
    match version {
        b'3' => {
            let (session_id, user_data_hash) = decode_v3(qr)?;
            Ok((3, session_id, user_data_hash))
        }
        b'4' => {
            let (orb_relay_id, app_authenticated_data_hash) = decode_v4(qr)?;
            Ok((4, orb_relay_id, app_authenticated_data_hash))
        }
        _ => Err(DecodeError::UnsupportedVersion),
    }
}

// the `decode_v3` method is specifically to decode user sessions where the id is the `session_id` and the hash is the hash from `UserData`
fn decode_v3(qr: &str) -> Result<(Uuid, Vec<u8>), DecodeError> {
    let Ok(payload) = BASE64_NOPAD.decode(&qr.as_bytes()[1..]) else {
        return Err(DecodeError::Base64);
    };
    let Some(session_id) = payload.get(0..16) else {
        return Err(DecodeError::Malformed);
    };
    let Some(user_data_hash) = payload.get(16..) else {
        return Err(DecodeError::Malformed);
    };
    let session_id = u128::from_be_bytes(session_id.try_into().unwrap());
    let session_id = Uuid::from_u128(session_id);
    Ok((session_id, user_data_hash.to_vec()))
}

// the `decode_v4` method is specifically to decode static sessions where the id is the `orb_relay_id` and the hash is the hash from `AppAuthenticatedData`
fn decode_v4(qr: &str) -> Result<(Uuid, Vec<u8>), DecodeError> {
    let Ok(payload) = BASE64_NOPAD.decode(&qr.as_bytes()[1..]) else {
        return Err(DecodeError::Base64);
    };
    let Some(orb_relay_id) = payload.get(0..16) else {
        return Err(DecodeError::Malformed);
    };
    let Some(app_authenticated_data_hash) = payload.get(16..) else {
        return Err(DecodeError::Malformed);
    };
    let orb_relay_id = u128::from_be_bytes(orb_relay_id.try_into().unwrap());
    let orb_relay_id = Uuid::from_u128(orb_relay_id);
    Ok((orb_relay_id, app_authenticated_data_hash.to_vec()))
}
