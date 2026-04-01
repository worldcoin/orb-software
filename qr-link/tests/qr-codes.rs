use data_encoding::BASE64_NOPAD;
use orb_qr_link::{decode_qr_with_version, encode_static_qr};
use orb_relay_messages::common::v1::AppAuthenticatedData;
use uuid::Uuid;

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
fn test_uuid_only_qr_decodes_but_verify_rejects() {
    let orb_relay_id = Uuid::nil();
    let payload = orb_relay_id.as_u128().to_be_bytes();
    let mut qr = String::from("4");
    BASE64_NOPAD.encode_append(&payload, &mut qr);

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
