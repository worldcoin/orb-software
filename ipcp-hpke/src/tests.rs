use super::*;
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

fn payload(fixture: &Value) -> IpcpImageHpkePayload {
    IpcpImageHpkePayload {
        enc: bytes(fixture, "enc"),
        ciphertext: bytes(fixture, "ct"),
    }
}

fn entropy(fixture: &Value) -> EphemeralEntropy {
    EphemeralEntropy {
        bytes: Zeroizing::new(bytes(fixture, "ikmE").try_into().unwrap()),
        position: 0,
    }
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
    assert_eq!(
        f["qr_modes"],
        serde_json::json!(["SHOW_TO_PAIR", "SCAN_TO_PAIR"])
    );
    let ipcp_image = bytes(&f, "pt");
    let payload = encrypt_with_entropy(&bytes(&f, "pkRm"), &ipcp_image, |output| {
        output.copy_from_slice(&bytes(&f, "ikmE"));
        Ok(())
    })
    .unwrap();
    assert_eq!(payload.enc, bytes(&f, "enc"));
    assert_eq!(payload.ciphertext, bytes(&f, "ct"));
    assert_eq!(
        payload.ciphertext,
        [bytes(&f, "ciphertext"), bytes(&f, "tag")].concat()
    );
    assert_eq!(
        payload.enc.len() + payload.ciphertext.len(),
        ipcp_image.len() + PAYLOAD_OVERHEAD
    );
    let packed = [payload.enc.as_slice(), payload.ciphertext.as_slice()].concat();
    assert_eq!(packed, bytes(&f, "encrypted_ipcp"));
    assert_eq!(
        decrypt_ipcp_image_payload(&bytes(&f, "skRm"), &payload)
            .unwrap()
            .as_slice(),
        ipcp_image
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
    assert_eq!(payload.enc, bytes(&f, "enc"));
    assert_eq!(payload.ciphertext, bytes(&f, "ct"));
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
fn system_entropy_roundtrips_ipcp_image_and_produces_fresh_encapsulation() {
    let f = ipcp_image_fixture();
    let public = bytes(&f, "pkRm");
    let private = bytes(&f, "skRm");
    for ipcp_image in [bytes(&f, "pt"), Vec::new()] {
        let first = encrypt_ipcp_image_payload(&public, &ipcp_image).unwrap();
        let second = encrypt_ipcp_image_payload(&public, &ipcp_image).unwrap();
        assert_ne!(first.enc, second.enc);
        for payload in [first, second] {
            assert_eq!(payload.enc.len(), KEY_LEN);
            assert_eq!(payload.ciphertext.len(), ipcp_image.len() + TAG_LEN);
            assert_eq!(
                decrypt_ipcp_image_payload(&private, &payload)
                    .unwrap()
                    .as_slice(),
                ipcp_image
            );
        }
    }
}

#[test]
fn wrong_recipient_and_invalid_key_lengths_are_rejected() {
    let f = ipcp_image_fixture();
    let payload = payload(&f);
    assert!(decrypt_ipcp_image_payload(&bytes(&f, "skEm"), &payload).is_err());
    for length in [0, 1, 31, 33, 64] {
        let key = vec![0xa5; length];
        assert!(matches!(
            decrypt_ipcp_image_payload(&key, &payload),
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
    let f = ipcp_image_fixture();
    for first_byte in [0, 1] {
        let mut public = [0; KEY_LEN];
        public[0] = first_byte;
        assert!(seal(&public, b"test", INFO, AAD, &mut entropy(&f)).is_err());
        let mut payload = payload(&f);
        payload.enc.copy_from_slice(&public);
        assert!(decrypt_ipcp_image_payload(&bytes(&f, "skRm"), &payload).is_err());
    }
}

#[test]
fn truncated_and_extended_payloads_are_rejected() {
    let f = ipcp_image_fixture();
    let private = bytes(&f, "skRm");
    let payload = payload(&f);
    for length in 0..KEY_LEN {
        let mut truncated = payload.clone();
        truncated.enc.truncate(length);
        assert!(matches!(
            decrypt_ipcp_image_payload(&private, &truncated),
            Err(Error::InvalidPayload)
        ));
    }
    let mut extended = payload.clone();
    extended.enc.push(0);
    assert!(matches!(
        decrypt_ipcp_image_payload(&private, &extended),
        Err(Error::InvalidPayload)
    ));
    for length in 0..payload.ciphertext.len() {
        let mut truncated = payload.clone();
        truncated.ciphertext.truncate(length);
        let result = decrypt_ipcp_image_payload(&private, &truncated);
        assert!(
            if length < TAG_LEN {
                matches!(result, Err(Error::InvalidPayload))
            } else {
                matches!(result, Err(Error::Decryption))
            },
            "ciphertext length {length}"
        );
    }
    let mut extended = payload;
    extended.ciphertext.push(0);
    assert!(matches!(
        decrypt_ipcp_image_payload(&private, &extended),
        Err(Error::Decryption)
    ));
}

#[test]
fn moving_bytes_across_payload_field_boundary_is_rejected() {
    let f = ipcp_image_fixture();
    let private = bytes(&f, "skRm");
    let original = payload(&f);
    let mut short_enc = original.clone();
    short_enc.ciphertext.insert(0, short_enc.enc.pop().unwrap());
    let mut long_enc = original;
    long_enc.enc.push(long_enc.ciphertext.remove(0));

    for malformed in [short_enc, long_enc] {
        assert_eq!(
            [malformed.enc.as_slice(), malformed.ciphertext.as_slice()].concat(),
            bytes(&f, "encrypted_ipcp")
        );
        assert!(matches!(
            decrypt_ipcp_image_payload(&private, &malformed),
            Err(Error::InvalidPayload)
        ));
    }
}

#[test]
fn modification_at_every_payload_byte_is_rejected() {
    let f = ipcp_image_fixture();
    let private = bytes(&f, "skRm");
    let payload = payload(&f);
    for index in 0..payload.enc.len() + payload.ciphertext.len() {
        let mut modified = payload.clone();
        if index < modified.enc.len() {
            modified.enc[index] ^= 1;
        } else {
            modified.ciphertext[index - modified.enc.len()] ^= 1;
        }
        assert!(
            decrypt_ipcp_image_payload(&private, &modified).is_err(),
            "byte {index}"
        );
    }
}

#[test]
fn incorrect_info_and_aad_are_rejected() {
    let f = ipcp_image_fixture();
    let private = bytes(&f, "skRm");
    let payload = payload(&f);
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
        assert!(decrypt_ipcp_image_payload(&private, &altered).is_err());
    }
}

#[test]
fn entropy_failure_returns_randomness_error() {
    let result = encrypt_with_entropy(
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
fn ipcp_image_and_entropy_use_zeroizing_guards() {
    fn assert_drop_guard<T: ZeroizeOnDrop>(_: &T) {}
    let f = ipcp_image_fixture();
    let mut ipcp_image =
        decrypt_ipcp_image_payload(&bytes(&f, "skRm"), &payload(&f)).unwrap();
    assert_drop_guard(&ipcp_image);
    assert!(!ipcp_image.is_empty());
    ipcp_image.as_mut_slice().zeroize();
    assert!(ipcp_image.iter().all(|&byte| byte == 0));
    let mut entropy = entropy(&f);
    assert_drop_guard(&entropy.bytes);
    entropy.bytes.zeroize();
    assert_eq!(*entropy.bytes, [0; KEY_LEN]);
}

#[test]
fn entropy_reads_advance_without_repeating_bytes() {
    let f = ipcp_image_fixture();
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
    let mut entropy = entropy(&ipcp_image_fixture());
    entropy.try_fill_bytes(&mut [0; KEY_LEN]).unwrap();
    entropy.try_fill_bytes(&mut [0; 1]).unwrap();
}
