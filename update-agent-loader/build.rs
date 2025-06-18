use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::RngCore;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // Only generate test keys in debug builds
    let profile = env::var("PROFILE").unwrap();
    if profile == "debug" {
        generate_test_keys();
    } else {
        // Also generate test keys if TEST_KEY is set, even in release builds
        // This is needed for running tests with --release
        if env::var("TEST_KEY").is_ok() {
            generate_test_keys();
        }
    }
}

fn generate_test_keys() {
    // Create directories if they don't exist
    let out_dir = env::var("OUT_DIR").unwrap();
    let keys_dir = Path::new(&out_dir).join("keys");
    fs::create_dir_all(&keys_dir).unwrap();

    let (public_key_bytes, secret_key_bytes) = if let Ok(test_key_path) =
        env::var("TEST_KEY")
    {
        // Use the file specified in TEST_KEY environment variable
        let secret_key_bytes =
            fs::read(&test_key_path).expect("Failed to read key from TEST_KEY path");

        // Convert the first 32 bytes to the required [u8; 32] array
        let mut key_array = [0u8; 32];
        let bytes_to_copy = std::cmp::min(32, secret_key_bytes.len());
        key_array[..bytes_to_copy].copy_from_slice(&secret_key_bytes[..bytes_to_copy]);

        let secret_key = ed25519_dalek::SigningKey::from_bytes(&key_array);
        let public_key = secret_key.verifying_key();
        (public_key.to_bytes(), secret_key.to_bytes())
    } else {
        // Generate a random seed for the key
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        let secret_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let public_key = secret_key.verifying_key();
        (public_key.to_bytes(), secret_key.to_bytes())
    };

    // Write the public key to the output directory
    fs::write(keys_dir.join("public_key.bin"), public_key_bytes)
        .expect("Failed to write public key");

    // Write the secret key to the output directory for tests
    fs::write(keys_dir.join("secret_key.bin"), secret_key_bytes)
        .expect("Failed to write secret key");

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
                .chain(public_key_bytes.iter())
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
                .chain(secret_key_bytes.iter())
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
    fs::write(debug_keys_dir.join("public_key.bin"), public_key_bytes)
        .expect("Failed to write public key to debug directory");
    fs::write(debug_keys_dir.join("secret_key.bin"), secret_key_bytes)
        .expect("Failed to write secret key to debug directory");

    // Write OpenSSL compatible PEM files
    fs::write(debug_keys_dir.join("public_key.pem"), public_key_pem)
        .expect("Failed to write PEM public key");
    fs::write(debug_keys_dir.join("private_key.pem"), private_key_pem)
        .expect("Failed to write PEM private key");

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rustc-env=PUBLIC_KEY_PATH={}",
        keys_dir.join("public_key.bin").display()
    )
}
