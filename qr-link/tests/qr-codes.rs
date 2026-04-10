use orb_qr_link::{decode_and_verify_qr, encode_static_qr, encode_static_qr_v5};
use orb_relay_messages::common::v1::AppAuthenticatedData;
use uuid::Uuid;

fn sample_data() -> AppAuthenticatedData {
    AppAuthenticatedData {
        identity_commitment: "0xabcd".to_string(),
        self_custody_public_key: "key".to_string(),
        pcp_version: 3,
        os: "Android".to_string(),
        os_version: "1.2.3".to_string(),
    }
}

// --- V4 (legacy BLAKE3 hash) ---

#[test]
fn test_v4_roundtrip() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();
    let qr = encode_static_qr(&orb_relay_id, app_data.hash(16));
    let (parsed_id, verified) = decode_and_verify_qr(&qr, &app_data).unwrap();
    assert_eq!(parsed_id, orb_relay_id);
    assert!(verified);
}

#[test]
fn test_v4_wrong_data_fails() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();
    let qr = encode_static_qr(&orb_relay_id, app_data.hash(16));

    let wrong_data = AppAuthenticatedData {
        identity_commitment: "0x9999".to_string(),
        ..sample_data()
    };
    let (_, verified) = decode_and_verify_qr(&qr, &wrong_data).unwrap();
    assert!(!verified);
}

#[test]
fn test_v4_different_pcp_version_fails() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();
    let qr = encode_static_qr(&orb_relay_id, app_data.hash(16));

    let different = AppAuthenticatedData {
        pcp_version: 99,
        ..app_data
    };
    let (_, verified) = decode_and_verify_qr(&qr, &different).unwrap();
    assert!(!verified);
}

#[test]
fn test_v4_corrupted_hash_fails() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();
    let mut hash = app_data.hash(16);
    hash[0] ^= 0xFF;
    let qr = encode_static_qr(&orb_relay_id, hash);
    let (_, verified) = decode_and_verify_qr(&qr, &app_data).unwrap();
    assert!(!verified);
}

#[test]
fn test_v4_empty_hash_rejects() {
    let orb_relay_id = Uuid::nil();
    let qr = encode_static_qr(&orb_relay_id, &[] as &[u8]);
    let (_, verified) = decode_and_verify_qr(&qr, &sample_data()).unwrap();
    assert!(!verified);
}

// --- V5 (length-prefixed BLAKE3 hash) ---

#[test]
fn test_v5_roundtrip() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();
    let qr = encode_static_qr_v5(&orb_relay_id, app_data.hash_with_length_prefix(16));
    let (parsed_id, verified) = decode_and_verify_qr(&qr, &app_data).unwrap();
    assert_eq!(parsed_id, orb_relay_id);
    assert!(verified);
}

#[test]
fn test_v5_wrong_data_fails() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();
    let qr = encode_static_qr_v5(&orb_relay_id, app_data.hash_with_length_prefix(16));

    let wrong_data = AppAuthenticatedData {
        identity_commitment: "0x9999".to_string(),
        ..sample_data()
    };
    let (_, verified) = decode_and_verify_qr(&qr, &wrong_data).unwrap();
    assert!(!verified);
}

#[test]
fn test_v5_empty_hash_rejects() {
    let orb_relay_id = Uuid::nil();
    let qr = encode_static_qr_v5(&orb_relay_id, &[] as &[u8]);
    let (_, verified) = decode_and_verify_qr(&qr, &sample_data()).unwrap();
    assert!(!verified);
}

// --- Cross-version rejection ---

#[test]
fn test_cross_version_hash_rejected() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();

    // v4 hash encoded as v5
    let qr = encode_static_qr_v5(&orb_relay_id, app_data.hash(16));
    let (_, verified) = decode_and_verify_qr(&qr, &app_data).unwrap();
    assert!(!verified);

    // v5 hash encoded as v4
    let qr = encode_static_qr(&orb_relay_id, app_data.hash_with_length_prefix(16));
    let (_, verified) = decode_and_verify_qr(&qr, &app_data).unwrap();
    assert!(!verified);
}

// --- Orb relay ID preservation ---

#[test]
fn test_roundtrip_preserves_orb_relay_id() {
    for _ in 0..10 {
        let id = Uuid::new_v4();
        let qr = encode_static_qr(&id, sample_data().hash(16));
        let (parsed_id, _) = decode_and_verify_qr(&qr, &sample_data()).unwrap();
        assert_eq!(id, parsed_id);
    }
}

// --- Error cases ---

#[test]
fn test_empty_qr_string_is_malformed() {
    assert!(decode_and_verify_qr("", &sample_data()).is_err());
}

#[test]
fn test_unsupported_version_is_rejected() {
    assert!(decode_and_verify_qr("3AAAA", &sample_data()).is_err());
}

#[test]
fn test_invalid_base64_is_rejected() {
    assert!(decode_and_verify_qr("4!!!", &sample_data()).is_err());
}
