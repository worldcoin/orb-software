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

fn orb_pairing_key_from_fixture(fixture: &Value) -> PairingKey {
    PairingKey::with_randomness(|output| {
        output.copy_from_slice(&bytes(fixture, "skRm"));
        Ok(())
    })
    .unwrap()
}

fn encrypted_ipcp_image_payload_from_fixture(fixture: &Value) -> IpcpImageHpkePayload {
    IpcpImageHpkePayload {
        enc: bytes(fixture, "enc"),
        ciphertext: bytes(fixture, "ct"),
    }
}

fn app_ephemeral_key_material_from_fixture(fixture: &Value) -> AppEphemeralKeyMaterial {
    AppEphemeralKeyMaterial {
        bytes: Zeroizing::new(bytes(fixture, "ikmE").try_into().unwrap()),
        position: 0,
    }
}

#[tokio::test]
async fn pairing_keys_are_fresh_and_decrypt_only_their_ipcp_image_payload() {
    let orb_pairing_key = PairingKey::new().unwrap();
    let other_orb_pairing_key = PairingKey::new().unwrap();
    assert_ne!(
        orb_pairing_key.public_key(),
        other_orb_pairing_key.public_key()
    );
    let orb_public_key_bytes = *orb_pairing_key.public_key();
    let ipcp_image = bytes(&ipcp_image_fixture(), "pt");
    let mut encrypted_ipcp_image_payload =
        encrypt_ipcp_image_payload(orb_public_key_bytes.to_vec(), ipcp_image.clone())
            .await
            .unwrap();
    assert_eq!(*orb_pairing_key.public_key(), orb_public_key_bytes);
    assert_eq!(
        orb_pairing_key
            .decrypt_ipcp_image_payload(&encrypted_ipcp_image_payload)
            .unwrap()
            .as_slice(),
        ipcp_image
    );
    assert!(matches!(
        other_orb_pairing_key.decrypt_ipcp_image_payload(&encrypted_ipcp_image_payload),
        Err(Error::Decryption)
    ));
    encrypted_ipcp_image_payload.ciphertext[0] ^= 1;
    assert!(matches!(
        orb_pairing_key.decrypt_ipcp_image_payload(&encrypted_ipcp_image_payload),
        Err(Error::Decryption)
    ));
}

#[test]
fn pairing_key_matches_existing_x25519_fixture() {
    let f = ipcp_image_fixture();
    let orb_pairing_key = orb_pairing_key_from_fixture(&f);
    assert_eq!(orb_pairing_key.public_key().as_slice(), bytes(&f, "pkRm"));
    assert_eq!(
        orb_pairing_key
            .decrypt_ipcp_image_payload(&encrypted_ipcp_image_payload_from_fixture(&f))
            .unwrap()
            .as_slice(),
        bytes(&f, "pt")
    );
}

#[tokio::test]
async fn encrypted_ipcp_image_payload_roundtrips_through_app_announcement() {
    let orb_pairing_key = PairingKey::new().unwrap();
    let ipcp_image = bytes(&ipcp_image_fixture(), "pt");
    let encrypted_ipcp_image_payload = encrypt_ipcp_image_payload(
        orb_pairing_key.public_key().to_vec(),
        ipcp_image.clone(),
    )
    .await
    .unwrap();
    let announcement = AnnounceAppId {
        encrypted_ipcp_payload: Some(encrypted_ipcp_image_payload),
        ..Default::default()
    };
    let decoded =
        AnnounceAppId::decode(announcement.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded, announcement);
    assert_eq!(
        orb_pairing_key
            .decrypt_ipcp_image_payload(&decoded.encrypted_ipcp_payload.unwrap())
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
    assert_eq!(bytes(&f, "info"), INFO);
    assert_eq!(bytes(&f, "aad"), AAD);
    let ipcp_image = bytes(&f, "pt");
    let encrypted_ipcp_image_payload =
        encrypt_ipcp_image_with_randomness(&bytes(&f, "pkRm"), &ipcp_image, |output| {
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
        orb_pairing_key_from_fixture(&f)
            .decrypt_ipcp_image_payload(&encrypted_ipcp_image_payload)
            .unwrap()
            .as_slice(),
        ipcp_image
    );
}

#[test]
fn published_cfrg_vector_matches() {
    let f = fixture(include_str!("fixtures/ipcp-hpke-cfrg.json"));
    let orb_pairing_key = orb_pairing_key_from_fixture(&f);
    let mut app_ephemeral_key_material = app_ephemeral_key_material_from_fixture(&f);
    let test_plaintext_bytes = bytes(&f, "pt");
    let encrypted_test_payload = encrypt_ipcp_image_with_context(
        &bytes(&f, "pkRm"),
        &test_plaintext_bytes,
        &bytes(&f, "info"),
        &bytes(&f, "aad"),
        &mut app_ephemeral_key_material,
    )
    .unwrap();
    assert_eq!(encrypted_test_payload.enc, bytes(&f, "enc"));
    assert_eq!(encrypted_test_payload.ciphertext, bytes(&f, "ct"));
    assert_eq!(app_ephemeral_key_material.position, KEY_LEN);
    let decrypted_test_plaintext_bytes = decrypt_ipcp_image_with_context(
        &orb_pairing_key.orb_private_key,
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

#[tokio::test(flavor = "current_thread")]
async fn system_randomness_roundtrips_ipcp_image_and_produces_fresh_app_public_key() {
    let f = ipcp_image_fixture();
    let orb_public_key_bytes = bytes(&f, "pkRm");
    let orb_pairing_key = orb_pairing_key_from_fixture(&f);
    for test_plaintext_bytes in [bytes(&f, "pt"), Vec::new()] {
        let first = tokio::spawn(encrypt_ipcp_image_payload(
            orb_public_key_bytes.clone(),
            test_plaintext_bytes.clone(),
        ))
        .await
        .unwrap()
        .unwrap();
        let second = encrypt_ipcp_image_payload(
            orb_public_key_bytes.clone(),
            test_plaintext_bytes.clone(),
        )
        .await
        .unwrap();
        assert_ne!(first.enc, second.enc);
        for encrypted_test_payload in [first, second] {
            assert_eq!(encrypted_test_payload.enc.len(), KEY_LEN);
            assert_eq!(
                encrypted_test_payload.ciphertext.len(),
                test_plaintext_bytes.len() + TAG_LEN
            );
            assert_eq!(
                orb_pairing_key
                    .decrypt_ipcp_image_payload(&encrypted_test_payload)
                    .unwrap()
                    .as_slice(),
                test_plaintext_bytes
            );
        }
    }
}

#[tokio::test]
async fn public_encryption_rejects_invalid_keys() {
    assert!(matches!(
        encrypt_ipcp_image_payload(vec![0xa5; KEY_LEN - 1], b"test".to_vec()).await,
        Err(Error::InvalidKey)
    ));
    assert!(matches!(
        encrypt_ipcp_image_payload(vec![0; KEY_LEN], b"test".to_vec()).await,
        Err(Error::Encryption)
    ));
}

#[test]
fn invalid_orb_public_key_lengths_are_rejected() {
    for length in [0, 1, 31, 33, 64] {
        let invalid_orb_public_key_bytes = vec![0xa5; length];
        assert!(matches!(
            encrypt_ipcp_image_with_randomness(
                &invalid_orb_public_key_bytes,
                b"test",
                |output| {
                    output.fill(1);
                    Ok(())
                }
            ),
            Err(Error::InvalidKey)
        ));
    }
}

#[test]
fn all_zero_and_low_order_public_keys_are_rejected() {
    let f = ipcp_image_fixture();
    let orb_pairing_key = orb_pairing_key_from_fixture(&f);
    for first_byte in [0, 1] {
        let mut invalid_public_key_bytes = [0; KEY_LEN];
        invalid_public_key_bytes[0] = first_byte;
        assert!(encrypt_ipcp_image_with_context(
            &invalid_public_key_bytes,
            b"test",
            INFO,
            AAD,
            &mut app_ephemeral_key_material_from_fixture(&f)
        )
        .is_err());
        let mut encrypted_ipcp_image_payload =
            encrypted_ipcp_image_payload_from_fixture(&f);
        encrypted_ipcp_image_payload
            .enc
            .copy_from_slice(&invalid_public_key_bytes);
        assert!(orb_pairing_key
            .decrypt_ipcp_image_payload(&encrypted_ipcp_image_payload)
            .is_err());
    }
}

#[test]
fn truncated_and_extended_payloads_are_rejected() {
    let f = ipcp_image_fixture();
    let orb_pairing_key = orb_pairing_key_from_fixture(&f);
    let encrypted_ipcp_image_payload = encrypted_ipcp_image_payload_from_fixture(&f);
    for length in 0..KEY_LEN {
        let mut truncated = encrypted_ipcp_image_payload.clone();
        truncated.enc.truncate(length);
        assert!(matches!(
            orb_pairing_key.decrypt_ipcp_image_payload(&truncated),
            Err(Error::InvalidPayload)
        ));
    }
    let mut extended = encrypted_ipcp_image_payload.clone();
    extended.enc.push(0);
    assert!(matches!(
        orb_pairing_key.decrypt_ipcp_image_payload(&extended),
        Err(Error::InvalidPayload)
    ));
    for length in 0..encrypted_ipcp_image_payload.ciphertext.len() {
        let mut truncated = encrypted_ipcp_image_payload.clone();
        truncated.ciphertext.truncate(length);
        let result = orb_pairing_key.decrypt_ipcp_image_payload(&truncated);
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
        orb_pairing_key.decrypt_ipcp_image_payload(&extended),
        Err(Error::Decryption)
    ));
}

#[test]
fn moving_bytes_across_payload_field_boundary_is_rejected() {
    let f = ipcp_image_fixture();
    let orb_pairing_key = orb_pairing_key_from_fixture(&f);
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
            orb_pairing_key.decrypt_ipcp_image_payload(&malformed),
            Err(Error::InvalidPayload)
        ));
    }
}

#[test]
fn modification_at_every_payload_byte_is_rejected() {
    let f = ipcp_image_fixture();
    let orb_pairing_key = orb_pairing_key_from_fixture(&f);
    let encrypted_ipcp_image_payload = encrypted_ipcp_image_payload_from_fixture(&f);
    for index in 0..encrypted_ipcp_image_payload.enc.len()
        + encrypted_ipcp_image_payload.ciphertext.len()
    {
        let mut modified = encrypted_ipcp_image_payload.clone();
        if index < modified.enc.len() {
            modified.enc[index] ^= 1;
        } else {
            modified.ciphertext[index - modified.enc.len()] ^= 1;
        }
        assert!(
            orb_pairing_key
                .decrypt_ipcp_image_payload(&modified)
                .is_err(),
            "byte {index}"
        );
    }
}

#[test]
fn incorrect_info_and_aad_are_rejected() {
    let f = ipcp_image_fixture();
    let orb_pairing_key = orb_pairing_key_from_fixture(&f);
    let encrypted_ipcp_image_payload = encrypted_ipcp_image_payload_from_fixture(&f);
    for (info, aad) in [
        (b"worldcoin/ipcp/hpke/v2".as_slice(), AAD),
        (INFO, b"unexpected metadata".as_slice()),
    ] {
        assert!(decrypt_ipcp_image_with_context(
            &orb_pairing_key.orb_private_key,
            &encrypted_ipcp_image_payload,
            info,
            aad
        )
        .is_err());
        let altered = encrypt_ipcp_image_with_context(
            &bytes(&f, "pkRm"),
            &bytes(&f, "pt"),
            info,
            aad,
            &mut app_ephemeral_key_material_from_fixture(&f),
        )
        .unwrap();
        assert!(orb_pairing_key
            .decrypt_ipcp_image_payload(&altered)
            .is_err());
    }
}

#[test]
fn encryption_randomness_failure_returns_randomness_error() {
    let result = encrypt_ipcp_image_with_randomness(
        &bytes(&ipcp_image_fixture(), "pkRm"),
        b"test",
        |output| {
            output.fill(0xa5);
            Err(getrandom::Error::UNSUPPORTED)
        },
    );
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
fn ipcp_image_and_app_key_material_use_zeroizing_guards() {
    fn assert_drop_guard<T: ZeroizeOnDrop>(_: &T) {}
    let f = ipcp_image_fixture();
    let mut decrypted_ipcp_image_bytes = orb_pairing_key_from_fixture(&f)
        .decrypt_ipcp_image_payload(&encrypted_ipcp_image_payload_from_fixture(&f))
        .unwrap();
    assert_drop_guard(&decrypted_ipcp_image_bytes);
    assert!(!decrypted_ipcp_image_bytes.is_empty());
    decrypted_ipcp_image_bytes.as_mut_slice().zeroize();
    assert!(decrypted_ipcp_image_bytes.iter().all(|&byte| byte == 0));
    let mut app_ephemeral_key_material = app_ephemeral_key_material_from_fixture(&f);
    assert_drop_guard(&app_ephemeral_key_material.bytes);
    app_ephemeral_key_material.bytes.zeroize();
    assert_eq!(*app_ephemeral_key_material.bytes, [0; KEY_LEN]);
}

#[test]
fn app_key_material_reads_advance_without_repeating_bytes() {
    let f = ipcp_image_fixture();
    let source = bytes(&f, "ikmE");
    let mut app_ephemeral_key_material = app_ephemeral_key_material_from_fixture(&f);
    assert_eq!(
        app_ephemeral_key_material.try_next_u32().unwrap(),
        u32::from_le_bytes(source[..4].try_into().unwrap())
    );
    assert_eq!(
        app_ephemeral_key_material.try_next_u64().unwrap(),
        u64::from_le_bytes(source[4..12].try_into().unwrap())
    );
    let mut remaining = [0; 20];
    app_ephemeral_key_material
        .try_fill_bytes(&mut remaining)
        .unwrap();
    assert_eq!(remaining, source[12..]);
    assert_eq!(app_ephemeral_key_material.position, KEY_LEN);
}

#[test]
#[should_panic(expected = "App key material exhausted")]
fn app_key_material_exhaustion_cannot_reuse_randomness() {
    let mut app_ephemeral_key_material =
        app_ephemeral_key_material_from_fixture(&ipcp_image_fixture());
    app_ephemeral_key_material
        .try_fill_bytes(&mut [0; KEY_LEN])
        .unwrap();
    app_ephemeral_key_material
        .try_fill_bytes(&mut [0; 1])
        .unwrap();
}
