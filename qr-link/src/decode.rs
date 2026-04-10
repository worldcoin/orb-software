use data_encoding::BASE64_NOPAD;
use orb_relay_messages::common::v1::AppAuthenticatedData;
use thiserror::Error;
use uuid::Uuid;

use crate::{QR_VERSION_4, QR_VERSION_5};

/// QR-code decoding/verification error.
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

fn decode_qr_with_version(qr: &str) -> Result<(u8, Uuid, Vec<u8>), DecodeError> {
    let Some(version) = qr.bytes().next() else {
        return Err(DecodeError::Malformed);
    };
    match version {
        b'4' | b'5' => {
            let (orb_relay_id, app_authenticated_data_hash) = decode_payload(qr)?;
            Ok((version - b'0', orb_relay_id, app_authenticated_data_hash))
        }
        _ => Err(DecodeError::UnsupportedVersion),
    }
}

/// Decodes a QR payload: 16-byte orb relay UUID followed by hash bytes.
fn decode_payload(qr: &str) -> Result<(Uuid, Vec<u8>), DecodeError> {
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

/// Decodes a QR-code string and verifies the `AppAuthenticatedData` hash
/// in one step. Returns the orb relay ID and whether the hash is valid.
pub fn decode_and_verify_qr(
    qr: &str,
    app_data: &AppAuthenticatedData,
) -> Result<(Uuid, bool), DecodeError> {
    let (version, orb_relay_id, hash) = decode_qr_with_version(qr)?;
    let verified = match version {
        QR_VERSION_4 => app_data.verify(&hash),
        QR_VERSION_5 => app_data.verify_with_length_prefix(&hash),
        _ => return Err(DecodeError::UnsupportedVersion),
    };

    Ok((orb_relay_id, verified))
}
