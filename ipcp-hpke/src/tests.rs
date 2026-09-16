use super::*;
use orb_relay_messages::{common::v1::AnnounceAppId, prost::Message};
use serde_json::Value;
use zeroize::{Zeroize, ZeroizeOnDrop};

fn fixture(source: &str) -> Value {
    serde_json::from_str(source).unwrap()
}

fn ipcp_image_fixture() -> Value {
    fixture(include_str!("fixtures/ipcp-hpke-v1.json"))
}

fn bytes(fixture: &Value, field: &str) -> Vec<u8> {
    hex::decode(fixture[field].as_str().unwrap()).unwrap()
}

fn pairing_key_from_fixture(fixture: &Value) -> PairingKey {
    PairingKey {
        pairing_private_key: <Profile as Kem>::PrivateKey::from_bytes(&bytes(
            fixture, "skRm",
        ))
        .unwrap(),
        pairing_public_key: bytes(fixture, "pkRm").try_into().unwrap(),
    }
}

fn encrypted_ipcp_image_payload_from_fixture(fixture: &Value) -> EncryptedPayload {
    EncryptedPayload {
        enc: bytes(fixture, "enc"),
        ciphertext: bytes(fixture, "ct"),
    }
}

#[test]
fn pairing_keys_are_fresh_and_decrypt_only_their_ipcp_image_payload() {
    let pairing_key = PairingKey::new_pairing_key();
    let other_pairing_key = PairingKey::new_pairing_key();
    assert_ne!(
        pairing_key.pairing_public_key,
        other_pairing_key.pairing_public_key
    );
    let pairing_public_key_bytes = pairing_key.pairing_public_key;
    let ipcp_image = bytes(&ipcp_image_fixture(), "pt");
    let mut encrypted_ipcp_image_payload =
        PairingKey::encrypt(&pairing_public_key_bytes, ipcp_image.clone()).unwrap();
    assert_eq!(pairing_key.pairing_public_key, pairing_public_key_bytes);
    assert_eq!(
        pairing_key
            .decrypt(&encrypted_ipcp_image_payload)
            .unwrap()
            .as_slice(),
        ipcp_image
    );
    assert!(matches!(
        other_pairing_key.decrypt(&encrypted_ipcp_image_payload),
        Err(Error::Decryption)
    ));
    encrypted_ipcp_image_payload.ciphertext[0] ^= 1;
    assert!(matches!(
        pairing_key.decrypt(&encrypted_ipcp_image_payload),
        Err(Error::Decryption)
    ));
}

#[test]
fn encrypted_ipcp_image_payload_roundtrips_through_app_announcement() {
    let pairing_key = PairingKey::new_pairing_key();
    let ipcp_image = bytes(&ipcp_image_fixture(), "pt");
    let encrypted_ipcp_image_payload =
        PairingKey::encrypt(&pairing_key.pairing_public_key, ipcp_image.clone())
            .unwrap();
    let announcement = AnnounceAppId {
        encrypted_ipcp_payload: Some(encrypted_ipcp_image_payload),
        ..Default::default()
    };
    let decoded =
        AnnounceAppId::decode(announcement.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, announcement);
    assert_eq!(
        pairing_key
            .decrypt(&decoded.encrypted_ipcp_payload.unwrap())
            .unwrap()
            .as_slice(),
        ipcp_image
    );
}

#[test]
fn ipcp_image_fixture_matches_decryption() {
    let f = ipcp_image_fixture();
    for (field, expected) in
        [("mode", 0), ("kem_id", 32), ("kdf_id", 1), ("aead_id", 2)]
    {
        assert_eq!(f[field].as_u64(), Some(expected), "{field}");
    }
    assert!(bytes(&f, "info").is_empty());
    assert!(bytes(&f, "aad").is_empty());
    let ipcp_image = bytes(&f, "pt");
    let encrypted_ipcp_image_payload = encrypted_ipcp_image_payload_from_fixture(&f);
    assert_eq!(
        encrypted_ipcp_image_payload.ciphertext,
        [bytes(&f, "ciphertext"), bytes(&f, "tag")].concat()
    );
    assert_eq!(
        encrypted_ipcp_image_payload.enc.len()
            + encrypted_ipcp_image_payload.ciphertext.len(),
        ipcp_image.len() + PAYLOAD_OVERHEAD
    );
    assert_eq!(
        pairing_key_from_fixture(&f)
            .decrypt(&encrypted_ipcp_image_payload)
            .unwrap()
            .as_slice(),
        ipcp_image
    );
}

#[test]
fn published_cfrg_vector_with_nonempty_context_is_rejected() {
    let f = fixture(include_str!("fixtures/ipcp-hpke-cfrg.json"));
    let pairing_key = pairing_key_from_fixture(&f);
    let encrypted_test_payload = encrypted_ipcp_image_payload_from_fixture(&f);
    assert!(!bytes(&f, "info").is_empty());
    assert!(!bytes(&f, "aad").is_empty());
    assert!(matches!(
        pairing_key.decrypt(&encrypted_test_payload),
        Err(Error::Decryption)
    ));
}

#[test]
fn system_randomness_roundtrips_with_fresh_ephemeral_public_keys() {
    let f = ipcp_image_fixture();
    let recipient_public_key = bytes(&f, "pkRm");
    let pairing_key = pairing_key_from_fixture(&f);
    for test_plaintext_bytes in [bytes(&f, "pt"), Vec::new()] {
        let first =
            PairingKey::encrypt(&recipient_public_key, test_plaintext_bytes.clone())
                .unwrap();
        let second =
            PairingKey::encrypt(&recipient_public_key, test_plaintext_bytes.clone())
                .unwrap();
        assert_ne!(first.enc, second.enc);
        for encrypted_test_payload in [first, second] {
            assert_eq!(encrypted_test_payload.enc.len(), KEY_LEN);
            assert_eq!(
                encrypted_test_payload.ciphertext.len(),
                test_plaintext_bytes.len() + TAG_LEN
            );
            assert_eq!(
                pairing_key
                    .decrypt(&encrypted_test_payload)
                    .unwrap()
                    .as_slice(),
                test_plaintext_bytes
            );
        }
    }
}

#[test]
fn pairing_key_rejects_invalid_recipient_public_keys() {
    assert!(matches!(
        PairingKey::encrypt(&[0xa5; KEY_LEN - 1], b"test".to_vec()),
        Err(Error::InvalidKey)
    ));
    assert!(matches!(
        PairingKey::encrypt(&[0; KEY_LEN], b"test".to_vec()),
        Err(Error::Encryption)
    ));
}

#[test]
fn all_zero_and_low_order_public_keys_are_rejected() {
    let f = ipcp_image_fixture();
    let pairing_key = pairing_key_from_fixture(&f);
    for first_byte in [0, 1] {
        let mut invalid_public_key_bytes = [0; KEY_LEN];
        invalid_public_key_bytes[0] = first_byte;
        assert!(
            PairingKey::encrypt(&invalid_public_key_bytes, b"test".to_vec()).is_err()
        );
        let mut encrypted_ipcp_image_payload =
            encrypted_ipcp_image_payload_from_fixture(&f);
        encrypted_ipcp_image_payload
            .enc
            .copy_from_slice(&invalid_public_key_bytes);
        assert!(pairing_key.decrypt(&encrypted_ipcp_image_payload).is_err());
    }
}

#[test]
fn truncated_and_extended_payloads_are_rejected() {
    let f = ipcp_image_fixture();
    let pairing_key = pairing_key_from_fixture(&f);
    let encrypted_ipcp_image_payload = encrypted_ipcp_image_payload_from_fixture(&f);
    for length in 0..KEY_LEN {
        let mut truncated = encrypted_ipcp_image_payload.clone();
        truncated.enc.truncate(length);
        assert!(matches!(
            pairing_key.decrypt(&truncated),
            Err(Error::InvalidPayload)
        ));
    }
    let mut extended = encrypted_ipcp_image_payload.clone();
    extended.enc.push(0);
    assert!(matches!(
        pairing_key.decrypt(&extended),
        Err(Error::InvalidPayload)
    ));
    for length in 0..encrypted_ipcp_image_payload.ciphertext.len() {
        let mut truncated = encrypted_ipcp_image_payload.clone();
        truncated.ciphertext.truncate(length);
        let result = pairing_key.decrypt(&truncated);
        assert!(
            if length < TAG_LEN {
                matches!(result, Err(Error::InvalidPayload))
            } else {
                matches!(result, Err(Error::Decryption))
            },
            "ciphertext length {length}"
        );
    }
    let mut extended = encrypted_ipcp_image_payload;
    extended.ciphertext.push(0);
    assert!(matches!(
        pairing_key.decrypt(&extended),
        Err(Error::Decryption)
    ));
}

#[test]
fn moving_bytes_across_payload_field_boundary_is_rejected() {
    let f = ipcp_image_fixture();
    let pairing_key = pairing_key_from_fixture(&f);
    let original = encrypted_ipcp_image_payload_from_fixture(&f);
    let original_field_bytes =
        [original.enc.as_slice(), original.ciphertext.as_slice()].concat();
    let mut short_enc = original.clone();
    short_enc.ciphertext.insert(0, short_enc.enc.pop().unwrap());
    let mut long_enc = original;
    long_enc.enc.push(long_enc.ciphertext.remove(0));

    for malformed in [short_enc, long_enc] {
        assert_eq!(
            [malformed.enc.as_slice(), malformed.ciphertext.as_slice()].concat(),
            original_field_bytes
        );
        assert!(matches!(
            pairing_key.decrypt(&malformed),
            Err(Error::InvalidPayload)
        ));
    }
}

#[test]
fn crypto_state_cleanup_guards_are_enabled() {
    fn assert_drop_guard<T: ZeroizeOnDrop>() {}

    assert_drop_guard::<aes_gcm::Aes256Gcm>();
    assert_drop_guard::<sha2_hpke::Sha256>();
    assert_drop_guard::<
        hmac::digest::block_api::Buffer<hmac::block_api::HmacCore<sha2_hpke::Sha256>>,
    >();
}

#[test]
fn decrypted_ipcp_image_uses_zeroizing_guard() {
    fn assert_drop_guard<T: ZeroizeOnDrop>(_: &T) {}
    let f = ipcp_image_fixture();
    let mut decrypted_ipcp_image_bytes = pairing_key_from_fixture(&f)
        .decrypt(&encrypted_ipcp_image_payload_from_fixture(&f))
        .unwrap();
    assert_drop_guard(&decrypted_ipcp_image_bytes);
    assert!(!decrypted_ipcp_image_bytes.is_empty());
    decrypted_ipcp_image_bytes.as_mut_slice().zeroize();
    assert!(decrypted_ipcp_image_bytes.iter().all(|&byte| byte == 0));
}
