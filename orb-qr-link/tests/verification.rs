use orb_qr_link::{decode_qr, encode_qr, DataPolicy, UserData};
use uuid::Uuid;

#[test]
fn test_encode_decode_verify() {
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
    };
    let qr = encode_qr(&session_id, user_data.hash(16));
    let (parsed_session_id, parsed_user_data_hash) = decode_qr(&qr).unwrap();
    assert_eq!(parsed_session_id, session_id);
    assert!(user_data.verify(parsed_user_data_hash));
}
