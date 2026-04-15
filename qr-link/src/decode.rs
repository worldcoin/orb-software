use data_encoding::BASE64_NOPAD;
use orb_relay_messages::common::v1::AppAuthenticatedData;
use thiserror::Error;
use uuid::Uuid;

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
        b'4' => {
            let (orb_relay_id, hash) = decode_payload(qr)?;
            Ok((4, orb_relay_id, hash))
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

/// Extracts just the orb relay UUID from a QR-code string without
/// verifying the hash.
pub fn decode_qr_uuid(qr: &str) -> Option<Uuid> {
    let version = qr.as_bytes().first()?;
    if *version != b'4' {
        return None;
    }
    let payload = BASE64_NOPAD.decode(&qr.as_bytes()[1..]).ok()?;
    let id = u128::from_be_bytes(payload.get(..16)?.try_into().ok()?);

    Some(Uuid::from_u128(id))
}

/// Decodes a QR-code string and returns the orb relay UUID and the raw
/// hash bytes without verifying.
pub fn decode_qr(qr: &str) -> Result<(Uuid, Vec<u8>), DecodeError> {
    let (_version, orb_relay_id, hash) = decode_qr_with_version(qr)?;
    Ok((orb_relay_id, hash))
}

/// Decodes a QR-code string and verifies the `AppAuthenticatedData` hash
/// in one step. Returns the orb relay ID and whether the hash is valid.
pub fn decode_and_verify_qr(
    qr: &str,
    app_data: &AppAuthenticatedData,
) -> Result<(Uuid, bool), DecodeError> {
    let (_version, orb_relay_id, hash) = decode_qr_with_version(qr)?;
    let verified = app_data.verify(&hash);

    Ok((orb_relay_id, verified))
}
