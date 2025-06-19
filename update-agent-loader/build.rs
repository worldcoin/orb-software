use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::RngCore;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Only generate test keys in debug builds
    let public_key_path = match env::var("UPDATE_AGENT_LOADER_PUBLIC_KEY") {
        Ok(path) => PathBuf::from(path),
        Err(env::VarError::NotPresent) => generate_test_keys(),
        Err(..) => panic!("Can't get path to public key from UPDATE_AGENT_LOADER_PUBLIC_KEY"),
    };
    println!(
        "cargo:rustc-env=PUBLIC_KEY_PATH={}",
        public_key_path.display()
    );
}

fn generate_test_keys() -> PathBuf {
    // Generate a random seed for the key
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let secret_key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let public_key = secret_key.verifying_key();

    // Generate OpenSSL compatible PEM format
    let public_key_pem = format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        BASE64
            .encode(
                // Ed25519 public key in OpenSSL format needs the OID prefix
                [
                    // SubjectPublicKeyInfo sequence
                    0x30, 0x2A, // Algorithm Identifier sequence
                    0x30, 0x05, // OID for Ed25519 (1.3.101.112)
                    0x06, 0x03, 0x2B, 0x65, 0x70, // BIT STRING tag
                    0x03, 0x21, 0x00,
                    // The actual key data
                ]
                .iter()
                .chain(public_key.to_bytes().iter())
                .copied()
                .collect::<Vec<u8>>()
            )
            .as_bytes()
            .chunks(64)
            .map(std::str::from_utf8)
            .collect::<Result<Vec<&str>, _>>()
            .unwrap()
            .join("\n")
    );

    let private_key_pem = format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        BASE64
            .encode(
                // Ed25519 private key in OpenSSL format (PKCS#8)
                [
                    // PrivateKeyInfo sequence
                    0x30, 0x2E, // Version (integer, value 0)
                    0x02, 0x01, 0x00, // Algorithm Identifier sequence
                    0x30, 0x05, // OID for Ed25519 (1.3.101.112)
                    0x06, 0x03, 0x2B, 0x65, 0x70,
                    // OCTET STRING containing the key
                    0x04, 0x22, // OCTET STRING containing the private key
                    0x04, 0x20,
                ]
                .iter()
                .chain(secret_key.to_bytes().iter())
                .copied()
                .collect::<Vec<u8>>()
            )
            .as_bytes()
            .chunks(64)
            .map(std::str::from_utf8)
            .collect::<Result<Vec<&str>, _>>()
            .unwrap()
            .join("\n")
    );

    // Write test keys to target/debug for easy access in tests
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let debug_keys_dir = Path::new(&manifest_dir)
        .join("target")
        .join("debug")
        .join("keys");
    fs::create_dir_all(&debug_keys_dir).expect("Failed to create debug keys directory");
    fs::write(debug_keys_dir.join("public_key.bin"), public_key.to_bytes())
        .expect("Failed to write public key to debug directory");
    fs::write(debug_keys_dir.join("secret_key.bin"), secret_key.to_bytes())
        .expect("Failed to write secret key to debug directory");

    // Write OpenSSL compatible PEM files
    fs::write(debug_keys_dir.join("public_key.pem"), public_key_pem)
        .expect("Failed to write PEM public key");

    fs::write(debug_keys_dir.join("private_key.pem"), private_key_pem)
        .expect("Failed to write PEM private key");

    debug_keys_dir.join("public_key.bin")
}
