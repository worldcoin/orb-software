use blake3::Hasher;
use serde::{Deserialize, Serialize};

const PCP_VERSION_DEFAULT: u16 = 2;

/// User's data to transfer from Worldcoin App to Orb.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UserData {
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
    /// Whether the orb should inform the app about if self-serve signup flow is enabled or not.
    #[serde(default = "default_false")]
    pub inform_app_of_self_serve_status: bool,
}

/// User's biometric data policy. Part of [`UserData`].
#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[repr(u8)]
pub enum DataPolicy {
    /// No images should be transmitted from the Orb.
    #[default]
    OptOut,
    /// Research and remote custody.
    FullDataOptIn,
}

impl UserData {
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
            inform_app_of_self_serve_status,
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
        if *inform_app_of_self_serve_status {
            hasher.update(&[true as u8]);
        }
    }
}

impl DataPolicy {
    /// Whether the policy is opt-in.
    #[must_use]
    pub fn is_opt_in(self) -> bool {
        match self {
            Self::OptOut => false,
            Self::FullDataOptIn => true,
        }
    }
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for DataPolicy {
    fn to_string(&self) -> String {
        match self {
            DataPolicy::FullDataOptIn => "full_data_opt_in".to_string(),
            DataPolicy::OptOut => "opt_out".to_string(),
        }
    }
}

const fn pcp_version_default() -> u16 {
    PCP_VERSION_DEFAULT
}

const fn default_false() -> bool {
    false
}
