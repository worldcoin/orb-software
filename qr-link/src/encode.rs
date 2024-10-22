use data_encoding::BASE64_NOPAD;
use uuid::Uuid;

/// QR-code version prefix.
pub const QR_VERSION: u8 = b'3';

/// Generates a QR-code string from `session_id` and `user_data_hash`.
pub fn encode_qr(session_id: &Uuid, user_data_hash: impl AsRef<[u8]>) -> String {
    let mut payload = Vec::new();
    payload.extend_from_slice(&session_id.as_u128().to_be_bytes());
    payload.extend_from_slice(user_data_hash.as_ref());

    let mut qr = String::new();
    qr.push(QR_VERSION.into());
    BASE64_NOPAD.encode_append(&payload, &mut qr);
    qr
}
