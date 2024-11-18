//! Handles loading and validating pubkeys.

use std::{array::TryFromSliceError, sync::OnceLock};

use ed25519_dalek::{SignatureError, VerifyingKey};
use hex_literal::hex;
use jose_jwk::Jwk;
use sha2::{Digest, Sha256};

const PROD_MANIFEST_PUBKEY: &str = include_str!("../../pubkeys/manifest.prod.json");
const PROD_MANIFEST_PUBKEY_SHA256: &[u8; 32] =
    &hex!("7f171578b473b356050418c0660cb9aaa2b67fe79d0a4c9fd3cf74e1195eb065");

const STAGE_MANIFEST_PUBKEY: &str = include_str!("../../pubkeys/manifest.stage.json");
const STAGE_MANIFEST_PUBKEY_SHA256: &[u8; 32] =
    &hex!("8410be3b790a432daadaa3bc8a243034815df96fc23e42785ccad5cd8d7512cc");

static PUBKEYS: OnceLock<ManifestPubkeys> = OnceLock::new();

#[derive(Debug)]
pub struct ManifestPubkeys {
    pub prod: VerifyingKey,
    pub stage: VerifyingKey,
}

/// Creates the different pubkeys that are allowed to be used.
pub fn get_pubkeys() -> &'static ManifestPubkeys {
    PUBKEYS.get_or_init(|| {
        let prod =
            make_key(PROD_MANIFEST_PUBKEY.as_bytes(), PROD_MANIFEST_PUBKEY_SHA256)
                .expect("prod pubkey failed validation");
        let stage = make_key(
            STAGE_MANIFEST_PUBKEY.as_bytes(),
            STAGE_MANIFEST_PUBKEY_SHA256,
        )
        .expect("stage pubkey failed validation");
        ManifestPubkeys { prod, stage }
    })
}

/// Deserializes `contents` into a public key, after verifying it against a checksum.
fn make_key(
    contents: &[u8],
    expected_sha256_checksum: &[u8; 32],
) -> Result<VerifyingKey, KeyValidationError> {
    let key_checksum = <Sha256 as Digest>::digest(contents);
    if key_checksum.as_slice() != expected_sha256_checksum {
        return Err(KeyValidationError::MismatchedChecksum);
    }

    let decoded_jwk: jose_jwk::Jwk = serde_json::from_slice(contents)?;
    let verifying_key = jwk_to_dalek(decoded_jwk)?;

    Ok(verifying_key)
}

#[derive(Debug, thiserror::Error)]
enum KeyValidationError {
    #[error("public key did not match the expected checksum")]
    MismatchedChecksum,
    #[error("not a json web key: {0}")]
    InvalidEncoding(#[from] serde_json::Error),
    #[error(transparent)]
    InvalidKey(#[from] JwkToDalekError),
}

#[derive(Debug, thiserror::Error)]
enum JwkToDalekError {
    #[error("the key uses a signing algo that we don't support")]
    UnsupportedKeyAlgo(Jwk),
    #[error("the key was supposed to be a public key, but encountered a private key instead")]
    UnexpectedPrivateKey(Jwk),
    #[error("invalid number of bytes in `.x` field of JWK")]
    InvalidBytes(#[from] TryFromSliceError),
    #[error("ed25519 public key failed validation: {0}")]
    InvalidPubKey(#[from] SignatureError),
}

/// Converts a [`Jwk`] to a [`ed25519_dalek::VerifyingKey`].
fn jwk_to_dalek(jwk: Jwk) -> Result<VerifyingKey, JwkToDalekError> {
    let jose_jwk::Key::Okp(ref okp) = jwk.key else {
        return Err(JwkToDalekError::UnsupportedKeyAlgo(jwk));
    };
    if okp.crv != jose_jwk::OkpCurves::Ed25519 {
        return Err(JwkToDalekError::UnsupportedKeyAlgo(jwk));
    }
    // Check secret field. Should be none for pub keys.
    if okp.d.is_some() {
        return Err(JwkToDalekError::UnexpectedPrivateKey(jwk));
    }
    let pubkey = ed25519_dalek::VerifyingKey::from_bytes(okp.x.as_ref().try_into()?)?;

    Ok(pubkey)
}

#[cfg(test)]
mod test {
    use base64::Engine as _;
    use jose_jwk::OkpCurves;

    use super::*;

    #[test]
    fn test_pinned_keys() {
        let ManifestPubkeys { prod, stage } = get_pubkeys();
        assert_ne!(prod, stage, "prod and stage keys were identical!");

        fn check_matching_keys(dalek: &VerifyingKey, encoded_jwk: &str) {
            let decoded_jwk: jose_jwk::Jwk = serde_json::from_str(encoded_jwk).unwrap();
            let jose_jwk::Key::Okp(jose_jwk::Okp { crv, x, d }) = decoded_jwk.key
            else {
                panic!("unexpected jwk");
            };
            assert_eq!(crv, OkpCurves::Ed25519);
            assert!(
                d.is_none(),
                "these are supposed to be pub keys, not priv keys"
            );
            assert_eq!(
                dalek.as_bytes(),
                x.as_ref(),
                "dalek key bytes didn't match jwk bytes"
            );
        }
        check_matching_keys(prod, PROD_MANIFEST_PUBKEY);
        check_matching_keys(stage, STAGE_MANIFEST_PUBKEY);
    }

    // Taken from https://github.com/NexusSocial/nexus-vr/blob/47f4dfe15f52228eb51c6646868c515018538764/apps/identity_server/src/jwk.rs#L29
    #[test]
    fn pub_jwk_test_vectors() {
        // arrange
        // See https://datatracker.ietf.org/doc/html/rfc8037#appendix-A.2
        let rfc_example = serde_json::json! ({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"
        });
        let pubkey_bytes = hex_literal::hex!(
            "d7 5a 98 01 82 b1 0a b7 d5 4b fe d3 c9 64 07 3a
            0e e1 72 f3 da a6 23 25 af 02 1a 68 f7 07 51 1a"
        );
        assert_eq!(
            base64::prelude::BASE64_URL_SAFE_NO_PAD
                .decode(rfc_example["x"].as_str().unwrap())
                .unwrap(),
            pubkey_bytes,
            "sanity check: example bytes should match, they come from the RFC itself"
        );
        let jwk: Jwk = serde_json::from_value(rfc_example).unwrap();

        // act
        let verifying_key = jwk_to_dalek(jwk).expect("failed to convert to dalek key");

        // assert
        assert_eq!(verifying_key.as_bytes(), &pubkey_bytes);
        // TODO: Check that validating sig with a JWKS libary also works with dalek.
    }
}
