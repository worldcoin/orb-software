use data_encoding::BASE64_NOPAD;
use uuid::Uuid;

use crate::{QR_VERSION_4, QR_VERSION_5};

/// Generates a QR-code (V4) string using the legacy hash format.
pub fn encode_static_qr(
    orb_relay_id: &Uuid,
    authenticated_data_hash: impl AsRef<[u8]>,
) -> String {
    encode_qr(QR_VERSION_4, orb_relay_id, authenticated_data_hash)
}

/// Generates a QR-code (V5) string using the length-prefixed hash format.
pub fn encode_static_qr_v5(
    orb_relay_id: &Uuid,
    authenticated_data_hash: impl AsRef<[u8]>,
) -> String {
    encode_qr(QR_VERSION_5, orb_relay_id, authenticated_data_hash)
}

fn encode_qr(
    version: u8,
    orb_relay_id: &Uuid,
    authenticated_data_hash: impl AsRef<[u8]>,
) -> String {
    let mut payload = Vec::new();
    payload.extend_from_slice(&orb_relay_id.as_u128().to_be_bytes());
    payload.extend_from_slice(authenticated_data_hash.as_ref());

    let mut qr = String::new();
    qr.push((b'0' + version) as char);
    BASE64_NOPAD.encode_append(&payload, &mut qr);

    qr
}
