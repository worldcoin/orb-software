use orb_info::OrbId;
use serde::{Deserialize, Serialize};

use crate::Nonce;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Proof {
    pub orb_id: OrbId,
    pub server_nonce: Nonce,
    /// If provided, it is combined with server_nonce for freshness
    pub orb_nonce: Option<Nonce>,
    /// This is the new, fuse-backed session key
    pub jetson_authkey: KeyInfo,
    /// This is the new migrated attestation key. The old legacy key may or may not
    /// still be in use.
    pub attestation_key: KeyInfo,
    /// This is the new migrated iris key. The old legacy key may or may not still be in
    /// use.
    pub iris_code_key: KeyInfo,
}

/// Key info. NOTE: exact representation should essentially the same as the keys for
/// orb-attest in mongo.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    /// PEM format pubkey
    pub key: String,
    #[serde(with = "crate::base64_serde")]
    pub signature: Vec<u8>,
    #[serde(with = "crate::base64_serde")]
    pub extra_data: Vec<u8>,
    // active: bool,
}
