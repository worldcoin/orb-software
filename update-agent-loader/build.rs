use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::RngCore;
use std::env;
use std::fs;

fn main() {
    // This package is Linux-only, skip build on other platforms
    if !cfg!(target_os = "linux") {
        println!("cargo:warning=update-agent-loader is only supported on Linux, skipping build");
        return;
    }

    println!("cargo:rerun-if-env-changed=UPDATE_AGENT_LOADER_PUBLIC_KEY");
    let public_key = match env::var("UPDATE_AGENT_LOADER_PUBLIC_KEY") {
        Ok(path) => {
            let d = fs::read(path).unwrap();
            d.try_into().unwrap()
        }
        Err(env::VarError::NotPresent) => generate_test_pubkey(),
        Err(..) => {
            panic!("Can't get path to public key from UPDATE_AGENT_LOADER_PUBLIC_KEY")
        }
    };
    println!(
        "cargo:rustc-env=PUBLIC_KEY_BASE64={}",
        BASE64.encode(public_key)
    );
}

/// Generates an ED25519 key-pair and *drops* the private key, only pubkey
/// returns a pubkey. This makes the key unusable, but that is intentional, if you want to
/// use the software, bring your ouwn keys.
fn generate_test_pubkey() -> [u8; 32] {
    // Generate a random seed for the key
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let secret_key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let public_key = secret_key.verifying_key();

    public_key.to_bytes()
}
