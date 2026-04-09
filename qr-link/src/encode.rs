use data_encoding::BASE64_NOPAD;
use uuid::Uuid;

/// QR-code version prefix for legacy hash.
pub const QR_VERSION_V4: u8 = b'4';

/// QR-code version prefix for length-prefixed hash.
pub const QR_VERSION_V5: u8 = b'5';

/// Generates a QR-code (V4) string using the legacy hash format.
pub fn encode_static_qr(
    orb_relay_id: &Uuid,
    authenticated_data_hash: impl AsRef<[u8]>,
) -> String {
    encode_qr(QR_VERSION_V4, orb_relay_id, authenticated_data_hash)
}

/// Generates a QR-code (V5) string using the length-prefixed hash format.
pub fn encode_static_qr_v5(
    orb_relay_id: &Uuid,
    authenticated_data_hash: impl AsRef<[u8]>,
) -> String {
    encode_qr(QR_VERSION_V5, orb_relay_id, authenticated_data_hash)
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
    qr.push(version.into());
    BASE64_NOPAD.encode_append(&payload, &mut qr);
    qr
}
