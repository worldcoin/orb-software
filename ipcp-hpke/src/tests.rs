use super::*;
use orb_relay_messages::{common::v1::AnnounceAppId, prost::Message};
use serde_json::Value;
use zeroize::{Zeroize, ZeroizeOnDrop};

const IPCP_INFO: &[u8] = b"worldcoin/ipcp/hpke/v1";
const IPCP_AAD: &[u8] = b"";

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
    PairingKey::with_randomness(|output| {
        output.copy_from_slice(&bytes(fixture, "skRm"));
        Ok(())
    })
    .unwrap()
}

fn encrypted_ipcp_image_payload_from_fixture(fixture: &Value) -> EncryptedPayload {
    EncryptedPayload {
        enc: bytes(fixture, "enc"),
        ciphertext: bytes(fixture, "ct"),
    }
}

fn ephemeral_key_material_from_fixture(fixture: &Value) -> EphemeralKeyMaterial {
    EphemeralKeyMaterial {
        bytes: Zeroizing::new(bytes(fixture, "ikmE").try_into().unwrap()),
        position: 0,
    }
}

#[test]
fn pairing_keys_are_fresh_and_decrypt_only_their_ipcp_image_payload() {
    let pairing_key = PairingKey::new().unwrap();
    let other_pairing_key = PairingKey::new().unwrap();
    assert_ne!(
        pairing_key.pairing_public_key(),
        other_pairing_key.pairing_public_key()
    );
    let pairing_public_key_bytes = *pairing_key.pairing_public_key();
    let recipient_key =
        RecipientKey::from_public_key(&pairing_public_key_bytes).unwrap();
    let ipcp_image = bytes(&ipcp_image_fixture(), "pt");
    let mut encrypted_ipcp_image_payload = recipient_key
        .encrypt(ipcp_image.clone(), IPCP_INFO, IPCP_AAD)
        .unwrap();
    assert_eq!(*pairing_key.pairing_public_key(), pairing_public_key_bytes);
    assert_eq!(
        pairing_key
            .decrypt(&encrypted_ipcp_image_payload, IPCP_INFO, IPCP_AAD)
            .unwrap()
            .as_slice(),
        ipcp_image
    );
    assert!(matches!(
        other_pairing_key.decrypt(&encrypted_ipcp_image_payload, IPCP_INFO, IPCP_AAD),
        Err(Error::Decryption)
    ));
    encrypted_ipcp_image_payload.ciphertext[0] ^= 1;
    assert!(matches!(
        pairing_key.decrypt(&encrypted_ipcp_image_payload, IPCP_INFO, IPCP_AAD),
        Err(Error::Decryption)
    ));
}

#[test]
fn encrypted_ipcp_image_payload_roundtrips_through_app_announcement() {
    let pairing_key = PairingKey::new().unwrap();
    let recipient_key =
        RecipientKey::from_public_key(pairing_key.pairing_public_key()).unwrap();
    let ipcp_image = bytes(&ipcp_image_fixture(), "pt");
    let encrypted_ipcp_image_payload = recipient_key
        .encrypt(ipcp_image.clone(), IPCP_INFO, IPCP_AAD)
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
            .decrypt(
                &decoded.encrypted_ipcp_payload.unwrap(),
                IPCP_INFO,
                IPCP_AAD
            )
            .unwrap()
            .as_slice(),
        ipcp_image
    );
}

#[test]
fn pairing_key_randomness_failure_returns_randomness_error() {
    assert!(matches!(
        PairingKey::with_randomness(|output| {
            output.fill(0xa5);
            Err(getrandom::Error::UNSUPPORTED)
        }),
        Err(Error::Randomness)
    ));
}

#[test]
fn ipcp_image_fixture_matches_encryption_and_decryption() {
    let f = ipcp_image_fixture();
    for (field, expected) in
        [("mode", 0), ("kem_id", 32), ("kdf_id", 1), ("aead_id", 2)]
    {
        assert_eq!(f[field].as_u64(), Some(expected), "{field}");
    }
    assert_eq!(bytes(&f, "info"), IPCP_INFO);
    assert_eq!(bytes(&f, "aad"), IPCP_AAD);
    let recipient_key = RecipientKey::from_public_key(&bytes(&f, "pkRm")).unwrap();
    let ipcp_image = bytes(&f, "pt");
    let encrypted_ipcp_image_payload = recipient_key
        .encrypt_with_randomness(&ipcp_image, IPCP_INFO, IPCP_AAD, |output| {
            output.copy_from_slice(&bytes(&f, "ikmE"));
            Ok(())
        })
        .unwrap();
    assert_eq!(encrypted_ipcp_image_payload.enc, bytes(&f, "enc"));
    assert_eq!(encrypted_ipcp_image_payload.ciphertext, bytes(&f, "ct"));
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
            .decrypt(&encrypted_ipcp_image_payload, IPCP_INFO, IPCP_AAD)
            .unwrap()
            .as_slice(),
        ipcp_image
    );
}

#[test]
fn published_cfrg_vector_matches() {
    let f = fixture(include_str!("fixtures/ipcp-hpke-cfrg.json"));
    let pairing_key = pairing_key_from_fixture(&f);
    let recipient_key = RecipientKey::from_public_key(&bytes(&f, "pkRm")).unwrap();
    let mut ephemeral_key_material = ephemeral_key_material_from_fixture(&f);
    let test_plaintext_bytes = bytes(&f, "pt");
    let encrypted_test_payload = recipient_key
        .encrypt_with_context(
            &test_plaintext_bytes,
            &bytes(&f, "info"),
            &bytes(&f, "aad"),
            &mut ephemeral_key_material,
        )
        .unwrap();
    assert_eq!(encrypted_test_payload.enc, bytes(&f, "enc"));
    assert_eq!(encrypted_test_payload.ciphertext, bytes(&f, "ct"));
    assert_eq!(ephemeral_key_material.position, KEY_LEN);
    let decrypted_test_plaintext_bytes = pairing_key
        .decrypt(
            &encrypted_test_payload,
            &bytes(&f, "info"),
            &bytes(&f, "aad"),
        )
        .unwrap();
    assert_eq!(
        decrypted_test_plaintext_bytes.as_slice(),
        test_plaintext_bytes
    );
}

#[test]
fn system_randomness_roundtrips_protocol_contexts_with_fresh_ephemeral_public_keys() {
    let f = ipcp_image_fixture();
    let recipient_key = RecipientKey::from_public_key(&bytes(&f, "pkRm")).unwrap();
    let pairing_key = pairing_key_from_fixture(&f);
    for (info, aad) in [
        (IPCP_INFO, IPCP_AAD),
        (b"another/protocol/v1".as_slice(), b"session:42".as_slice()),
    ] {
        for test_plaintext_bytes in [bytes(&f, "pt"), Vec::new()] {
            let first = recipient_key
                .encrypt(test_plaintext_bytes.clone(), info, aad)
                .unwrap();
            let second = recipient_key
                .encrypt(test_plaintext_bytes.clone(), info, aad)
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
                        .decrypt(&encrypted_test_payload, info, aad)
                        .unwrap()
                        .as_slice(),
                    test_plaintext_bytes
                );
                if info != IPCP_INFO {
                    assert!(matches!(
                        pairing_key.decrypt(
                            &encrypted_test_payload,
                            IPCP_INFO,
                            IPCP_AAD
                        ),
                        Err(Error::Decryption)
                    ));
                }
            }
        }
    }
}

#[test]
fn recipient_key_rejects_invalid_keys() {
    assert!(matches!(
        RecipientKey::from_public_key(&[0xa5; KEY_LEN - 1]),
        Err(Error::InvalidKey)
    ));
    assert!(matches!(
        RecipientKey::from_public_key(&[0; KEY_LEN])
            .unwrap()
            .encrypt(b"test".to_vec(), IPCP_INFO, IPCP_AAD),
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
        let recipient_key =
            RecipientKey::from_public_key(&invalid_public_key_bytes).unwrap();
        assert!(recipient_key
            .encrypt_with_context(
                b"test",
                IPCP_INFO,
                IPCP_AAD,
                &mut ephemeral_key_material_from_fixture(&f)
            )
            .is_err());
        let mut encrypted_ipcp_image_payload =
            encrypted_ipcp_image_payload_from_fixture(&f);
        encrypted_ipcp_image_payload
            .enc
            .copy_from_slice(&invalid_public_key_bytes);
        assert!(pairing_key
            .decrypt(&encrypted_ipcp_image_payload, IPCP_INFO, IPCP_AAD)
            .is_err());
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
            pairing_key.decrypt(&truncated, IPCP_INFO, IPCP_AAD),
            Err(Error::InvalidPayload)
        ));
    }
    let mut extended = encrypted_ipcp_image_payload.clone();
    extended.enc.push(0);
    assert!(matches!(
        pairing_key.decrypt(&extended, IPCP_INFO, IPCP_AAD),
        Err(Error::InvalidPayload)
    ));
    for length in 0..encrypted_ipcp_image_payload.ciphertext.len() {
        let mut truncated = encrypted_ipcp_image_payload.clone();
        truncated.ciphertext.truncate(length);
        let result = pairing_key.decrypt(&truncated, IPCP_INFO, IPCP_AAD);
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
        pairing_key.decrypt(&extended, IPCP_INFO, IPCP_AAD),
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
            pairing_key.decrypt(&malformed, IPCP_INFO, IPCP_AAD),
            Err(Error::InvalidPayload)
        ));
    }
}

#[test]
fn incorrect_info_and_aad_are_rejected() {
    let f = ipcp_image_fixture();
    let pairing_key = pairing_key_from_fixture(&f);
    let recipient_key = RecipientKey::from_public_key(&bytes(&f, "pkRm")).unwrap();
    let encrypted_ipcp_image_payload = encrypted_ipcp_image_payload_from_fixture(&f);
    for (info, aad) in [
        (b"worldcoin/ipcp/hpke/v2".as_slice(), IPCP_AAD),
        (IPCP_INFO, b"unexpected metadata".as_slice()),
    ] {
        assert!(pairing_key
            .decrypt(&encrypted_ipcp_image_payload, info, aad)
            .is_err());
        let altered = recipient_key
            .encrypt_with_context(
                &bytes(&f, "pt"),
                info,
                aad,
                &mut ephemeral_key_material_from_fixture(&f),
            )
            .unwrap();
        assert!(pairing_key.decrypt(&altered, IPCP_INFO, IPCP_AAD).is_err());
    }
}

#[test]
fn encryption_randomness_failure_returns_randomness_error() {
    let recipient_key =
        RecipientKey::from_public_key(&bytes(&ipcp_image_fixture(), "pkRm")).unwrap();
    let result =
        recipient_key.encrypt_with_randomness(b"test", IPCP_INFO, IPCP_AAD, |output| {
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
        hmac::digest::block_api::Buffer<hmac::block_api::HmacCore<sha2_hpke::Sha256>>,
    >();
}

#[test]
fn ipcp_image_and_ephemeral_key_material_use_zeroizing_guards() {
    fn assert_drop_guard<T: ZeroizeOnDrop>(_: &T) {}
    let f = ipcp_image_fixture();
    let mut decrypted_ipcp_image_bytes = pairing_key_from_fixture(&f)
        .decrypt(
            &encrypted_ipcp_image_payload_from_fixture(&f),
            IPCP_INFO,
            IPCP_AAD,
        )
        .unwrap();
    assert_drop_guard(&decrypted_ipcp_image_bytes);
    assert!(!decrypted_ipcp_image_bytes.is_empty());
    decrypted_ipcp_image_bytes.as_mut_slice().zeroize();
    assert!(decrypted_ipcp_image_bytes.iter().all(|&byte| byte == 0));
    let mut ephemeral_key_material = ephemeral_key_material_from_fixture(&f);
    assert_drop_guard(&ephemeral_key_material.bytes);
    ephemeral_key_material.bytes.zeroize();
    assert_eq!(*ephemeral_key_material.bytes, [0; KEY_LEN]);
}

#[test]
fn ephemeral_key_material_reads_advance_without_repeating_bytes() {
    let f = ipcp_image_fixture();
    let source = bytes(&f, "ikmE");
    let mut ephemeral_key_material = ephemeral_key_material_from_fixture(&f);
    assert_eq!(
        ephemeral_key_material.try_next_u32().unwrap(),
        u32::from_le_bytes(source[..4].try_into().unwrap())
    );
    assert_eq!(
        ephemeral_key_material.try_next_u64().unwrap(),
        u64::from_le_bytes(source[4..12].try_into().unwrap())
    );
    let mut remaining = [0; 20];
    ephemeral_key_material
        .try_fill_bytes(&mut remaining)
        .unwrap();
    assert_eq!(remaining, source[12..]);
    assert_eq!(ephemeral_key_material.position, KEY_LEN);
}

#[test]
#[should_panic(expected = "Ephemeral key material exhausted")]
fn ephemeral_key_material_exhaustion_cannot_reuse_randomness() {
    let mut ephemeral_key_material =
        ephemeral_key_material_from_fixture(&ipcp_image_fixture());
    ephemeral_key_material
        .try_fill_bytes(&mut [0; KEY_LEN])
        .unwrap();
    ephemeral_key_material.try_fill_bytes(&mut [0; 1]).unwrap();
}
