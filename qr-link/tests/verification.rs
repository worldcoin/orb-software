use orb_qr_link::{
    decode_qr, decode_qr_with_version, encode_qr, encode_static_qr,
    AppAuthenticatedDataExt, DataPolicy, UserData,
};
use orb_relay_messages::common::v1::AppAuthenticatedData;
use uuid::Uuid;

#[test]
fn test_encode_decode_verify_without_age_verification_token() {
    let session_id = Uuid::new_v4();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let identity_commitment = "0xabcd";
    let user_data = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: DataPolicy::OptOut,
        pcp_version: 3,
        user_centric_signup: true,
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: None,
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(parsed_user_data_hash));
}

#[test]
fn test_encode_decode_verify_with_age_verification_token() {
    let session_id = Uuid::new_v4();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let sample_jwt_token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let identity_commitment = "0xabcd";
    let user_data = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: DataPolicy::OptOut,
        pcp_version: 3,
        user_centric_signup: true,
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: Some(sample_jwt_token.to_string()),
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(parsed_user_data_hash));
    let (version, parsed_session_id_v4, parsed_user_data_hash_v4) =
        decode_qr_with_version(&qr).unwrap();
    assert_eq!(version, 3);
    assert_eq!(parsed_session_id_v4, session_id);
    assert!(user_data.verify(parsed_user_data_hash_v4));
}

#[test]
fn test_encode_decode_static() {
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
    assert!(app_data.verify(parsed_app_data));
}
