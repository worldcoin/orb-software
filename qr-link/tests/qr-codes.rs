use orb_qr_link::{decode_qr_with_version, encode_static_qr, encode_static_qr_v5};
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

#[test]
fn test_encode_decode() {
    let orb_relay_id = "11111111-1111-1111-1111-111111111111"
        .parse::<Uuid>()
        .unwrap();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let identity_commitment = "0xabcd";
    let app_data = AppAuthenticatedData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        pcp_version: 3,
        os: "Android".to_string(),
        os_version: "1.2.3".to_string(),
    };
    let hash_app_data = app_data.hash(16);
    let qr = encode_static_qr(&orb_relay_id, hash_app_data);
    let (version, parsed_orb_relay_id, parsed_app_data) =
        decode_qr_with_version(&qr).unwrap();
    assert_eq!(version, 4);
    assert_eq!(parsed_orb_relay_id, orb_relay_id);
    assert!(app_data.verify(parsed_app_data));
}
#[test]
fn test_encode_decode_failure() {
    let orb_relay_id = Uuid::new_v4();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let identity_commitment = "0xabcd";
    let app_data = AppAuthenticatedData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        pcp_version: 3,
        os: "Android".to_string(),
        os_version: "1.2.3".to_string(),
    };
    let hash_app_data = app_data.hash(16);
    let qr = encode_static_qr(&orb_relay_id, hash_app_data);
    let (version, parsed_orb_relay_id, parsed_app_data) =
        decode_qr_with_version(&qr).unwrap();
    assert_eq!(version, 4);
    assert_eq!(parsed_orb_relay_id, orb_relay_id);
    let incorrect_app_data = AppAuthenticatedData {
        identity_commitment: "0x1234".to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        pcp_version: 2,
        os: "Android".to_string(),
        os_version: "1.2.3".to_string(),
    };
    assert!(!incorrect_app_data.verify(parsed_app_data));
}

#[test]
fn test_empty_hash_qr_decodes_but_verify_rejects() {
    let orb_relay_id = Uuid::nil();
    let qr = encode_static_qr(&orb_relay_id, &[] as &[u8]);

    // decode_v4 accepts a 16-byte payload (empty hash slice)
    let (version, parsed_id, hash) = decode_qr_with_version(&qr).unwrap();
    assert_eq!(version, 4);
    assert_eq!(parsed_id, orb_relay_id);
    assert!(hash.is_empty());

    // but verify() rejects an empty hash
    let app_data = AppAuthenticatedData {
        identity_commitment: "0xabcd".to_string(),
        self_custody_public_key: "key".to_string(),
        pcp_version: 3,
        os: "Android".to_string(),
        os_version: "1.2.3".to_string(),
    };
    assert!(!app_data.verify(hash));
}

#[test]
fn test_different_pcp_version_fails_verify() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = AppAuthenticatedData {
        identity_commitment: "0xabcd".to_string(),
        self_custody_public_key: "key".to_string(),
        pcp_version: 3,
        os: "Android".to_string(),
        os_version: "1.2.3".to_string(),
    };
    let hash = app_data.hash(16);
    let qr = encode_static_qr(&orb_relay_id, hash);
    let (_, _, parsed_hash) = decode_qr_with_version(&qr).unwrap();

    let different_app_data = AppAuthenticatedData {
        pcp_version: 99,
        ..app_data
    };
    assert!(!different_app_data.verify(parsed_hash));
}

#[test]
fn test_corrupted_hash_fails_verify() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = AppAuthenticatedData {
        identity_commitment: "0xabcd".to_string(),
        self_custody_public_key: "key".to_string(),
        pcp_version: 3,
        os: "Android".to_string(),
        os_version: "1.2.3".to_string(),
    };
    let mut hash = app_data.hash(16);
    hash[0] ^= 0xFF;
    let qr = encode_static_qr(&orb_relay_id, hash);
    let (_, _, parsed_hash) = decode_qr_with_version(&qr).unwrap();
    assert!(!app_data.verify(parsed_hash));
}

#[test]
fn test_empty_qr_string_is_malformed() {
    assert!(decode_qr_with_version("").is_err());
}

#[test]
fn test_unsupported_version_is_rejected() {
    assert!(decode_qr_with_version("3AAAA").is_err());
}

#[test]
fn test_invalid_base64_is_rejected() {
    // '4' is valid version prefix, but '!!!' is not valid base64
    assert!(decode_qr_with_version("4!!!").is_err());
}

#[test]
fn test_roundtrip_preserves_orb_relay_id() {
    for _ in 0..10 {
        let id = Uuid::new_v4();
        let qr = encode_static_qr(&id, [0xAB; 16]);
        let (_, parsed_id, _) = decode_qr_with_version(&qr).unwrap();
        assert_eq!(id, parsed_id);
    }
}

// --- V5 (length-prefixed hash) tests ---

#[test]
fn test_v5_encode_decode_roundtrip() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();
    let hash = app_data.hash_with_length_prefix(16);
    let qr = encode_static_qr_v5(&orb_relay_id, hash);
    let (version, parsed_id, parsed_hash) =
        decode_qr_with_version(&qr).unwrap();
    assert_eq!(version, 5);
    assert_eq!(parsed_id, orb_relay_id);
    assert!(app_data.verify_with_length_prefix(parsed_hash));
}

#[test]
fn test_v5_rejects_wrong_data() {
    let orb_relay_id = Uuid::new_v4();
    let app_data = sample_data();
    let hash = app_data.hash_with_length_prefix(16);
    let qr = encode_static_qr_v5(&orb_relay_id, hash);
    let (_, _, parsed_hash) = decode_qr_with_version(&qr).unwrap();

    let wrong_data = AppAuthenticatedData {
        identity_commitment: "0x9999".to_string(),
        ..sample_data()
    };
    assert!(!wrong_data.verify_with_length_prefix(parsed_hash));
}

#[test]
fn test_v5_hash_not_accepted_by_legacy_verify() {
    let app_data = sample_data();
    let v5_hash = app_data.hash_with_length_prefix(16);
    assert!(!app_data.verify(&v5_hash));
}

#[test]
fn test_v4_hash_not_accepted_by_v5_verify() {
    let app_data = sample_data();
    let v4_hash = app_data.hash(16);
    assert!(!app_data.verify_with_length_prefix(&v4_hash));
}

#[test]
fn test_v5_empty_hash_rejected() {
    let orb_relay_id = Uuid::nil();
    let qr = encode_static_qr_v5(&orb_relay_id, &[] as &[u8]);
    let (version, _, hash) = decode_qr_with_version(&qr).unwrap();
    assert_eq!(version, 5);
    assert!(!sample_data().verify_with_length_prefix(hash));
}
