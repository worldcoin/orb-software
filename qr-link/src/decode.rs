use data_encoding::BASE64_NOPAD;
use orb_relay_messages::common::v1::AppAuthenticatedData;
use thiserror::Error;
use uuid::Uuid;

use crate::{QR_VERSION_4, QR_VERSION_5};

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
        b'4' | b'5' => {
            let (orb_relay_id, app_authenticated_data_hash) = decode_payload(qr)?;
            Ok((version - b'0', orb_relay_id, app_authenticated_data_hash))
        }
        _ => Err(DecodeError::UnsupportedVersion),
    }
}

/// Decodes a QR payload: 16-byte orb relay UUID followed by hash bytes.
/// The wire format (UUID + hash bytes) is the same for v4 and v5, but the
/// hash bytes differ because v5 uses a length-prefixed BLAKE3 hash.
/// The caller must use the version from [`decode_qr_with_version`] to pick
/// the matching verify method.
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

/// Error returned by [`verify_qr`] for unrecognized QR versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("unsupported QR version: {0}")]
pub struct UnsupportedVersion(pub u8);

/// Verifies an `AppAuthenticatedData` hash using the verification method
/// corresponding to the given QR version.
pub fn verify_qr(
    app_data: &AppAuthenticatedData,
    hash: &[u8],
    version: u8,
) -> Result<bool, UnsupportedVersion> {
    match version {
        QR_VERSION_4 => Ok(app_data.verify(hash)),
        QR_VERSION_5 => Ok(app_data.verify_with_length_prefix(hash)),
        _ => Err(UnsupportedVersion(version)),
    }
}
