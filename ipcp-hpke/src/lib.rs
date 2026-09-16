#![forbid(unsafe_code)]

use std::convert::Infallible;

use hpke::{
    aead::{AeadTag, AesGcm256},
    kdf::HkdfSha256,
    kem::X25519HkdfSha256,
    Deserializable, Kem, OpModeR, OpModeS, Serializable,
};
use rand_core::{TryCryptoRng, TryRng};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

type Profile = X25519HkdfSha256;

const INFO: &[u8] = b"worldcoin/ipcp/hpke/v1";
const AAD: &[u8] = b"";
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
pub const PAYLOAD_OVERHEAD: usize = KEY_LEN + TAG_LEN;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid X25519 key")]
    InvalidKey,
    #[error("Invalid encrypted iPCP payload")]
    InvalidPayload,
    #[error("System randomness unavailable")]
    Randomness,
    #[error("iPCP encryption failed")]
    Encryption,
    #[error("iPCP decryption failed")]
    Decryption,
}

pub fn encrypt_ipcp(orb_public_key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    encrypt_with_entropy(orb_public_key, plaintext, getrandom::fill)
}

fn encrypt_with_entropy(
    orb_public_key: &[u8],
    plaintext: &[u8],
    fill: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
) -> Result<Vec<u8>, Error> {
    let mut entropy = EphemeralEntropy {
        bytes: Zeroizing::new([0; KEY_LEN]),
        position: 0,
    };
    fill(entropy.bytes.as_mut()).map_err(|_| Error::Randomness)?;
    seal(orb_public_key, plaintext, INFO, AAD, &mut entropy)
}

fn seal(
    orb_public_key: &[u8],
    plaintext: &[u8],
    info: &[u8],
    aad: &[u8],
    entropy: &mut EphemeralEntropy,
) -> Result<Vec<u8>, Error> {
    let public_key = <Profile as Kem>::PublicKey::from_bytes(orb_public_key)
        .map_err(|_| Error::InvalidKey)?;
    let len = plaintext
        .len()
        .checked_add(PAYLOAD_OVERHEAD)
        .ok_or(Error::InvalidPayload)?;
    let tag_start = len - TAG_LEN;
    let mut payload = Zeroizing::new(vec![0; len]);
    payload[KEY_LEN..tag_start].copy_from_slice(plaintext);
    let (enc, tag) = hpke::single_shot_seal_inout_detached_with_rng::<
        AesGcm256,
        HkdfSha256,
        Profile,
    >(
        &OpModeS::Base,
        &public_key,
        info,
        payload[KEY_LEN..tag_start].as_mut().into(),
        aad,
        entropy,
    )
    .map_err(|_| Error::Encryption)?;
    payload[..KEY_LEN].copy_from_slice(&enc.to_bytes());
    payload[tag_start..].copy_from_slice(&tag.to_bytes());
    Ok(std::mem::take(&mut *payload))
}

pub fn decrypt_ipcp(
    orb_private_key: &[u8],
    encrypted_ipcp: &[u8],
) -> Result<Zeroizing<Vec<u8>>, Error> {
    open(orb_private_key, encrypted_ipcp, INFO, AAD)
}

fn open(
    orb_private_key: &[u8],
    encrypted_ipcp: &[u8],
    info: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, Error> {
    if encrypted_ipcp.len() < PAYLOAD_OVERHEAD {
        return Err(Error::InvalidPayload);
    }
    let tag_start = encrypted_ipcp.len() - TAG_LEN;
    let private_key = <Profile as Kem>::PrivateKey::from_bytes(orb_private_key)
        .map_err(|_| Error::InvalidKey)?;
    let enc = <Profile as Kem>::EncappedKey::from_bytes(&encrypted_ipcp[..KEY_LEN])
        .map_err(|_| Error::InvalidPayload)?;
    let tag = AeadTag::<AesGcm256>::from_bytes(&encrypted_ipcp[tag_start..])
        .map_err(|_| Error::InvalidPayload)?;
    let mut plaintext = Zeroizing::new(encrypted_ipcp[KEY_LEN..tag_start].to_vec());
    hpke::single_shot_open_inout_detached::<AesGcm256, HkdfSha256, Profile>(
        &OpModeR::Base,
        &private_key,
        &enc,
        info,
        plaintext.as_mut_slice().into(),
        aad,
        &tag,
    )
    .map_err(|_| Error::Decryption)?;
    Ok(plaintext)
}

pub fn encrypted_ipcp_hash(encrypted_ipcp: &[u8]) -> [u8; 32] {
    Sha256::digest(encrypted_ipcp).into()
}

struct EphemeralEntropy {
    bytes: Zeroizing<[u8; KEY_LEN]>,
    position: usize,
}

impl TryRng for EphemeralEntropy {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut bytes = Zeroizing::new([0; 4]);
        self.try_fill_bytes(bytes.as_mut())?;
        Ok(u32::from_le_bytes(*bytes))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut bytes = Zeroizing::new([0; 8]);
        self.try_fill_bytes(bytes.as_mut())?;
        Ok(u64::from_le_bytes(*bytes))
    }

    fn try_fill_bytes(&mut self, output: &mut [u8]) -> Result<(), Infallible> {
        let end = self
            .position
            .checked_add(output.len())
            .expect("Entropy overflow");
        output.copy_from_slice(
            self.bytes
                .get(self.position..end)
                .expect("X25519 entropy exhausted"),
        );
        self.position = end;
        Ok(())
    }
}

impl TryCryptoRng for EphemeralEntropy {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use zeroize::{Zeroize, ZeroizeOnDrop};

    fn fixture(source: &str) -> Value {
        serde_json::from_str(source).unwrap()
    }

    fn ipcp_fixture() -> Value {
        fixture(include_str!("fixtures/ipcp-hpke-v1.json"))
    }

    fn bytes(fixture: &Value, field: &str) -> Vec<u8> {
        hex::decode(fixture[field].as_str().unwrap()).unwrap()
    }

    fn entropy(fixture: &Value) -> EphemeralEntropy {
        EphemeralEntropy {
            bytes: Zeroizing::new(bytes(fixture, "ikmE").try_into().unwrap()),
            position: 0,
        }
    }

    #[test]
    fn ipcp_fixture_matches_encryption_decryption_and_hash() {
        let f = ipcp_fixture();
        for (field, expected) in
            [("mode", 0), ("kem_id", 32), ("kdf_id", 1), ("aead_id", 2)]
        {
            assert_eq!(f[field].as_u64(), Some(expected), "{field}");
        }
        assert_eq!(bytes(&f, "info"), INFO);
        assert_eq!(bytes(&f, "aad"), AAD);
        assert_eq!(
            f["qr_modes"],
            serde_json::json!(["SHOW_TO_PAIR", "SCAN_TO_PAIR"])
        );
        let plaintext = bytes(&f, "pt");
        let payload = encrypt_with_entropy(&bytes(&f, "pkRm"), &plaintext, |output| {
            output.copy_from_slice(&bytes(&f, "ikmE"));
            Ok(())
        })
        .unwrap();
        assert_eq!(payload, bytes(&f, "encrypted_ipcp"));
        assert_eq!(payload, [bytes(&f, "enc"), bytes(&f, "ct")].concat());
        assert_eq!(
            payload,
            [bytes(&f, "enc"), bytes(&f, "ciphertext"), bytes(&f, "tag")].concat()
        );
        assert_eq!(payload.len(), plaintext.len() + PAYLOAD_OVERHEAD);
        assert_eq!(
            encrypted_ipcp_hash(&payload).as_slice(),
            bytes(&f, "encrypted_ipcp_hash")
        );
        assert_eq!(
            decrypt_ipcp(&bytes(&f, "skRm"), &payload)
                .unwrap()
                .as_slice(),
            plaintext
        );
    }

    #[test]
    fn published_cfrg_vector_matches() {
        let f = fixture(include_str!("fixtures/ipcp-hpke-cfrg.json"));
        let mut entropy = entropy(&f);
        let payload = seal(
            &bytes(&f, "pkRm"),
            &bytes(&f, "pt"),
            &bytes(&f, "info"),
            &bytes(&f, "aad"),
            &mut entropy,
        )
        .unwrap();
        assert_eq!(payload, [bytes(&f, "enc"), bytes(&f, "ct")].concat());
        assert_eq!(entropy.position, KEY_LEN);
        let plaintext = open(
            &bytes(&f, "skRm"),
            &payload,
            &bytes(&f, "info"),
            &bytes(&f, "aad"),
        )
        .unwrap();
        assert_eq!(plaintext.as_slice(), bytes(&f, "pt"));
    }

    #[test]
    fn system_entropy_roundtrips_and_produces_fresh_encapsulation() {
        let f = ipcp_fixture();
        let public = bytes(&f, "pkRm");
        let private = bytes(&f, "skRm");
        for plaintext in [bytes(&f, "pt"), Vec::new()] {
            let first = encrypt_ipcp(&public, &plaintext).unwrap();
            let second = encrypt_ipcp(&public, &plaintext).unwrap();
            assert_ne!(first[..KEY_LEN], second[..KEY_LEN]);
            for payload in [first, second] {
                assert_eq!(payload.len(), plaintext.len() + PAYLOAD_OVERHEAD);
                assert_eq!(
                    decrypt_ipcp(&private, &payload).unwrap().as_slice(),
                    plaintext
                );
            }
        }
    }

    #[test]
    fn wrong_recipient_and_invalid_key_lengths_are_rejected() {
        let f = ipcp_fixture();
        let payload = bytes(&f, "encrypted_ipcp");
        assert!(decrypt_ipcp(&bytes(&f, "skEm"), &payload).is_err());
        for length in [0, 1, 31, 33, 64] {
            let key = vec![0xa5; length];
            assert!(matches!(
                decrypt_ipcp(&key, &payload),
                Err(Error::InvalidKey)
            ));
            assert!(matches!(
                encrypt_with_entropy(&key, b"test", |output| {
                    output.fill(1);
                    Ok(())
                }),
                Err(Error::InvalidKey)
            ));
        }
    }

    #[test]
    fn all_zero_and_low_order_public_keys_are_rejected() {
        let f = ipcp_fixture();
        for first_byte in [0, 1] {
            let mut public = [0; KEY_LEN];
            public[0] = first_byte;
            assert!(seal(&public, b"test", INFO, AAD, &mut entropy(&f)).is_err());
            let mut payload = bytes(&f, "encrypted_ipcp");
            payload[..KEY_LEN].copy_from_slice(&public);
            assert!(decrypt_ipcp(&bytes(&f, "skRm"), &payload).is_err());
        }
    }

    #[test]
    fn truncated_and_extended_payloads_are_rejected() {
        let f = ipcp_fixture();
        let private = bytes(&f, "skRm");
        let mut payload = bytes(&f, "encrypted_ipcp");
        for length in 0..payload.len() {
            assert!(
                decrypt_ipcp(&private, &payload[..length]).is_err(),
                "length {length}"
            );
        }
        payload.push(0);
        assert!(decrypt_ipcp(&private, &payload).is_err());
    }

    #[test]
    fn modification_at_every_payload_byte_is_rejected() {
        let f = ipcp_fixture();
        let private = bytes(&f, "skRm");
        let payload = bytes(&f, "encrypted_ipcp");
        for index in 0..payload.len() {
            let mut modified = payload.clone();
            modified[index] ^= 1;
            assert!(decrypt_ipcp(&private, &modified).is_err(), "byte {index}");
            assert_ne!(
                encrypted_ipcp_hash(&modified),
                encrypted_ipcp_hash(&payload)
            );
        }
    }

    #[test]
    fn incorrect_info_and_aad_are_rejected() {
        let f = ipcp_fixture();
        let private = bytes(&f, "skRm");
        let payload = bytes(&f, "encrypted_ipcp");
        for (info, aad) in [
            (b"worldcoin/ipcp/hpke/v2".as_slice(), AAD),
            (INFO, b"unexpected metadata".as_slice()),
        ] {
            assert!(open(&private, &payload, info, aad).is_err());
            let altered = seal(
                &bytes(&f, "pkRm"),
                &bytes(&f, "pt"),
                info,
                aad,
                &mut entropy(&f),
            )
            .unwrap();
            assert!(decrypt_ipcp(&private, &altered).is_err());
        }
    }

    #[test]
    fn entropy_failure_returns_randomness_error() {
        let result =
            encrypt_with_entropy(&bytes(&ipcp_fixture(), "pkRm"), b"test", |output| {
                output.fill(0xa5);
                Err(getrandom::Error::UNSUPPORTED)
            });
        assert!(matches!(result, Err(Error::Randomness)));
    }

    #[test]
    fn crypto_state_cleanup_guards_are_enabled() {
        fn assert_drop_guard<T: ZeroizeOnDrop>() {}

        assert_drop_guard::<aes_gcm::Aes256Gcm>();
        assert_drop_guard::<sha2_hpke::Sha256>();
        assert_drop_guard::<
            hmac::digest::block_api::Buffer<
                hmac::block_api::HmacCore<sha2_hpke::Sha256>,
            >,
        >();
    }

    #[test]
    fn plaintext_and_entropy_use_zeroizing_guards() {
        fn assert_drop_guard<T: ZeroizeOnDrop>(_: &T) {}
        let f = ipcp_fixture();
        let mut plaintext =
            decrypt_ipcp(&bytes(&f, "skRm"), &bytes(&f, "encrypted_ipcp")).unwrap();
        assert_drop_guard(&plaintext);
        assert!(!plaintext.is_empty());
        plaintext.as_mut_slice().zeroize();
        assert!(plaintext.iter().all(|&byte| byte == 0));
        let mut entropy = entropy(&f);
        assert_drop_guard(&entropy.bytes);
        entropy.bytes.zeroize();
        assert_eq!(*entropy.bytes, [0; KEY_LEN]);
    }

    #[test]
    fn entropy_reads_advance_without_repeating_bytes() {
        let f = ipcp_fixture();
        let source = bytes(&f, "ikmE");
        let mut entropy = entropy(&f);
        assert_eq!(
            entropy.try_next_u32().unwrap(),
            u32::from_le_bytes(source[..4].try_into().unwrap())
        );
        assert_eq!(
            entropy.try_next_u64().unwrap(),
            u64::from_le_bytes(source[4..12].try_into().unwrap())
        );
        let mut remaining = [0; 20];
        entropy.try_fill_bytes(&mut remaining).unwrap();
        assert_eq!(remaining, source[12..]);
        assert_eq!(entropy.position, KEY_LEN);
    }

    #[test]
    #[should_panic(expected = "X25519 entropy exhausted")]
    fn entropy_exhaustion_cannot_reuse_randomness() {
        let mut entropy = entropy(&ipcp_fixture());
        entropy.try_fill_bytes(&mut [0; KEY_LEN]).unwrap();
        entropy.try_fill_bytes(&mut [0; 1]).unwrap();
    }
}
