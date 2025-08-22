use blake3::Hasher;
use orb_qr_link::{decode_qr, encode_qr, DataPolicy, UserData};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Define constant needed for OldUserData
const PCP_VERSION_DEFAULT: u16 = 2;

fn pcp_version_default() -> u16 {
    PCP_VERSION_DEFAULT
}

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

#[test]
fn test_backwards_compatibility_orb_new_version_app_old() {
    // in this scenario the orb is the first to get the new version and the app still uses the old one
    // this is copied from oxide
    let self_custody_public_key = "pub_key_wabba_ducky";
    let identity_commitment = "0xabcd";
    let orb_relay_id = "123432";
    let bypass_age_token = "token";
    let old_user_data = OldUserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: DataPolicy::OptOut,
        pcp_version: 2,
        user_centric_signup: true,
        orb_relay_app_id: Some(orb_relay_id.to_string()),
        bypass_age_verification_token: Some(bypass_age_token.to_string()),
    };
    let hash_from_app = old_user_data.hash(16);

    // Here's the JSON string from the signup service
    let json_string_from_signup_service = format!(
        r#"{{"selfCustodyPublicKey":"{self_custody_public_key}","identityCommitment":"{identity_commitment}","dataPolicy":"OPT_OUT","userCentricSignup":true,"bypassAgeVerificationToken":"{bypass_age_token}","orbRelayAppId":"{orb_relay_id}"}}"#
    );

    // In a real implementation with serde_json added to Cargo.toml, you would:
    let user_data_from_json: UserData =
        serde_json::from_str(&json_string_from_signup_service)
            .expect("Failed to parse UserData from JSON");

    // Verify that the converted UserData has the expected values
    assert_eq!(user_data_from_json.hash(16), hash_from_app);
}

#[test]
fn test_backwards_compatibility_app_new_version_orb_old() {
    // in this scenario the app is the first to get the new version and the orb still uses the old one
    // this is copied from oxide
    let self_custody_public_key = "pub_key_wabba_ducky";
    let identity_commitment = "0xabcd";
    let orb_relay_id = "123432";
    let bypass_age_token = "token";
    let new_user_data = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: Some(DataPolicy::OptOut),
        pcp_version: 2,
        user_centric_signup: Some(true),
        orb_relay_app_id: Some(orb_relay_id.to_string()),
        bypass_age_verification_token: Some(bypass_age_token.to_string()),
    };
    let hash_from_app = new_user_data.hash(16);

    // Here's the JSON string from the signup service
    let json_string_from_signup_service = format!(
        r#"{{"selfCustodyPublicKey":"{self_custody_public_key}","identityCommitment":"{identity_commitment}","dataPolicy":"OPT_OUT","userCentricSignup":true,"bypassAgeVerificationToken":"{bypass_age_token}","orbRelayAppId":"{orb_relay_id}"}}"#
    );

    // In a real implementation with serde_json added to Cargo.toml, you would:
    let user_data_from_json: OldUserData =
        serde_json::from_str(&json_string_from_signup_service)
            .expect("Failed to parse UserData from JSON");

    // Verify that the converted UserData has the expected values
    assert_eq!(user_data_from_json.hash(16), hash_from_app);
}

#[test]
fn test_compatibility_for_app_and_orb_to_use_new_versions() {
    // this is copied from oxide
    let self_custody_public_key = "pub_key_wabba_ducky";
    let identity_commitment = "0xabcd";
    let user_data_app = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: None,
        pcp_version: 2,
        user_centric_signup: None,
        orb_relay_app_id: None,
        bypass_age_verification_token: None,
    };
    let hash_from_app = user_data_app.hash(16);

    // orb takes data from AppAnnounceId and creates user_data
    let user_data_orb = UserData {
        identity_commitment: identity_commitment.to_string(),
        self_custody_public_key: self_custody_public_key.to_string(),
        data_policy: None,
        pcp_version: 2,
        user_centric_signup: None,
        orb_relay_app_id: None,
        bypass_age_verification_token: None,
    };

    // Verify that the converted UserData has the expected values
    assert_eq!(user_data_orb.hash(16), hash_from_app);
}

/// User's data to transfer from Worldcoin App to Orb.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OldUserData {
    /// Identity commitment.
    pub identity_commitment: String,
    /// User's key stored in the app in the PEM public key format.
    pub self_custody_public_key: String,
    /// User's biometric data policy.
    pub data_policy: DataPolicy,
    /// Personal Custody Package version.
    #[serde(default = "pcp_version_default")]
    pub pcp_version: u16,
    /// Whether the orb should perform a app-centric signup.
    #[serde(default = "default_false")]
    pub user_centric_signup: bool,
    /// A unique UUID that the Orb will use to send messages to the app through Orb Relay.
    pub orb_relay_app_id: Option<String>,
    /// Whether the Orb should perform the age verification. If the token exists we skip the age verification.
    pub bypass_age_verification_token: Option<String>,
}
fn default_false() -> bool {
    false
}
impl OldUserData {
    /// Returns `true` if `hash` is a BLAKE3 hash of this [`UserData`].
    ///
    /// This method calculates its own hash of the same length as the input
    /// `hash` and checks if both hashes are identical.
    pub fn verify(&self, hash: impl AsRef<[u8]>) -> bool {
        let external_hash = hash.as_ref();
        let internal_hash = self.hash(external_hash.len());
        external_hash == internal_hash
    }

    /// Calculates a BLAKE3 hash of the length `n`.
    pub fn hash(&self, n: usize) -> Vec<u8> {
        let mut hasher = Hasher::new();
        self.hasher_update(&mut hasher);
        let mut output = vec![0; n];
        hasher.finalize_xof().fill(&mut output);
        output
    }

    // This method must hash every field.
    fn hasher_update(&self, hasher: &mut Hasher) {
        let Self {
            identity_commitment,
            self_custody_public_key,
            data_policy,
            pcp_version,
            user_centric_signup,
            orb_relay_app_id,
            bypass_age_verification_token,
        } = self;
        hasher.update(identity_commitment.as_bytes());
        hasher.update(self_custody_public_key.as_bytes());
        hasher.update(&[*data_policy as u8]);
        if *pcp_version != PCP_VERSION_DEFAULT {
            hasher.update(&pcp_version.to_ne_bytes());
        }
        if *user_centric_signup {
            hasher.update(&[true as u8]);
        }
        if let Some(app_id) = orb_relay_app_id {
            hasher.update(app_id.as_bytes());
        }
        if let Some(age_verification_token) = bypass_age_verification_token {
            hasher.update(age_verification_token.as_bytes());
        }
    }
}
