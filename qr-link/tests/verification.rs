use orb_qr_link::{decode_qr, encode_qr, DataPolicy, UserData};
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
        data_policy: Some(DataPolicy::OptOut),
        pcp_version: 3,
        user_centric_signup: Some(true),
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: None,
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(&parsed_user_data_hash));
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
        data_policy: Some(DataPolicy::OptOut),
        pcp_version: 3,
        user_centric_signup: Some(true),
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: Some(sample_jwt_token.to_string()),
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(&parsed_user_data_hash));
}

#[test]
fn test_with_optional_data_policy_none() {
    let session_id = Uuid::new_v4();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let identity_commitment = "0xabcd";
    let user_data = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: None,
        pcp_version: 3,
        user_centric_signup: Some(true),
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: None,
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(&parsed_user_data_hash));
}

#[test]
fn test_with_optional_user_centric_signup_none() {
    let session_id = Uuid::new_v4();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let identity_commitment = "0xabcd";
    let user_data = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: Some(DataPolicy::OptOut),
        pcp_version: 3,
        user_centric_signup: None,
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: None,
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(&parsed_user_data_hash));
}

#[test]
fn test_with_all_optional_fields_none() {
    let session_id = Uuid::new_v4();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let identity_commitment = "0xabcd";
    let user_data = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: None,
        pcp_version: 3,
        user_centric_signup: None,
        orb_relay_app_id: None,
        bypass_age_verification_token: None,
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(&parsed_user_data_hash));
}

#[test]
fn test_with_different_data_policy() {
    let session_id = Uuid::new_v4();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let identity_commitment = "0xabcd";
    let user_data = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: Some(DataPolicy::FullDataOptIn),
        pcp_version: 3,
        user_centric_signup: Some(true),
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: None,
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(&parsed_user_data_hash));

    // Ensure the hash verification fails if we change the data_policy
    let user_data_modified = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: Some(DataPolicy::OptOut),
        pcp_version: 3,
        user_centric_signup: Some(true),
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: None,
    };
    assert!(!user_data_modified.verify(&parsed_user_data_hash));
}

#[test]
fn test_hash_verification_with_default_pcp_version() {
    // Test the behavior with PCP_VERSION_DEFAULT (2)
    let session_id = Uuid::new_v4();
    let self_custody_public_key = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEA2boNBmJX4lGkA9kjthS5crXOBxu2BPycKRMakpzgLG4=
-----END PUBLIC KEY-----"#;
    let identity_commitment = "0xabcd";
    let user_data = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: Some(DataPolicy::OptOut),
        pcp_version: 2, // Default PCP version
        user_centric_signup: Some(true),
        orb_relay_app_id: Some("123123".to_string()),
        bypass_age_verification_token: None,
    };

    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(&parsed_user_data_hash));
}
