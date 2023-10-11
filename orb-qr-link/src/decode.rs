use data_encoding::BASE64_NOPAD;
use thiserror::Error;
use uuid::Uuid;

/// QR-code decoding error returned by [`decode_qr`].
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

/// Parses `session_id` and `user_data_hash` from a  QR-code string.
pub fn decode_qr(qr: &str) -> Result<(Uuid, Vec<u8>), DecodeError> {
    let Some(version) = qr.bytes().next() else { return Err(DecodeError::Malformed) };
    match version {
        b'3' => decode_v3(qr),
        _ => Err(DecodeError::UnsupportedVersion),
    }
}

fn decode_v3(qr: &str) -> Result<(Uuid, Vec<u8>), DecodeError> {
    let Ok(payload) = BASE64_NOPAD.decode(qr[1..].as_bytes()) else { return Err(DecodeError::Base64) };
    let Some(session_id) = payload.get(0..16) else { return Err(DecodeError::Malformed) };
    let Some(user_data_hash) = payload.get(16..) else { return Err(DecodeError::Malformed) };
    let session_id = u128::from_be_bytes(session_id.try_into().unwrap());
    let session_id = Uuid::from_u128(session_id);
    Ok((session_id, user_data_hash.to_vec()))
}
