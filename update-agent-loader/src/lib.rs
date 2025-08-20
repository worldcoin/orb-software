mod download;
mod memfile;
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
pub use download::download_and_execute;
use ed25519_dalek::{SigningKey, VerifyingKey};

const BUILD_TIME_PUBKEY: Option<&str> = option_env!("ORB_UPDATE_AGENT_LOADER_PUBKEY");
pub const BULD_TIME_PUBKEY_ENV_VAR: &str = "ORB_UPDATE_AGENT_LOADER_PUBKEY";

/// The global configuration for the program. Should be explicitly passed into spots
/// that need it, instead of grabbed globally.
#[derive(Debug, Clone)]
pub struct Config {
    pub public_key: VerifyingKey,
}

impl Config {
    /// Only `main` should ever call this. If you need the config, prefer instead
    /// passing it into your function as an argument / via dependency injection.
    /// This helps ensure that the code remains testable.
    pub fn from_env() -> Self {
        let vk: VerifyingKey = BUILD_TIME_PUBKEY
            .map(|b64| {
                let bytes = BASE64_URL_SAFE_NO_PAD
                    .decode(b64)
                    .expect("failed base64 decoding");

                VerifyingKey::from_bytes(
                    bytes
                        .as_slice()
                        .try_into()
                        .expect("invalid byte length for public key"),
                )
                .expect("invalid public key")
            })
            .unwrap_or_else(|| {
                SigningKey::generate(&mut rand::thread_rng()).verifying_key()
            });

        Self { public_key: vk }
    }
}
