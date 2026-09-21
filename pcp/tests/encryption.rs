use orb_pcp::encryption::{self, Error};
use sodiumoxide::crypto::{box_, sealedbox};

fn keypair() -> (box_::PublicKey, box_::SecretKey) {
    sodiumoxide::init().unwrap();
    box_::gen_keypair()
}

#[test]
fn encrypted_bytes_open_with_the_legacy_api() {
    let (public, secret) = keypair();
    for size in [0, 1, 256, 4096] {
        let plaintext: Vec<_> = (0_u8..=255).cycle().take(size).collect();
        let encrypted = encryption::seal(&plaintext, &public.0).unwrap();
        assert_eq!(encrypted.len(), plaintext.len() + 48);
        assert_eq!(
            sealedbox::open(&encrypted, &public, &secret).unwrap(),
            plaintext
        );
    }
}

#[test]
fn repeated_encryption_uses_fresh_ephemeral_keys() {
    let (public, secret) = keypair();
    let first = encryption::seal(b"synthetic", &public.0).unwrap();
    let second = encryption::seal(b"synthetic", &public.0).unwrap();
    assert_ne!(first, second);
    assert_ne!(&first[..32], &second[..32]);
    for ciphertext in [first, second] {
        assert_eq!(
            sealedbox::open(&ciphertext, &public, &secret).unwrap(),
            b"synthetic"
        );
    }
}

#[test]
fn wrong_recipient_tampering_and_truncation_fail_to_open() {
    let (public, secret) = keypair();
    let (other_public, other_secret) = keypair();
    let ciphertext = encryption::seal(b"synthetic", &public.0).unwrap();
    assert!(sealedbox::open(&ciphertext, &other_public, &other_secret).is_err());
    for i in 0..ciphertext.len() {
        let mut changed = ciphertext.clone();
        changed[i] ^= 1;
        assert!(sealedbox::open(&changed, &public, &secret).is_err());
    }
    for length in 0..ciphertext.len() {
        assert!(sealedbox::open(&ciphertext[..length], &public, &secret).is_err());
    }
}

#[test]
fn low_order_recipients_and_their_high_bit_aliases_are_rejected() {
    // Public low-order point encodings from libsodium's X25519 tests; no secret keys.
    let mut points = vec![[0; 32]];
    let mut one = [0; 32];
    one[0] = 1;
    points.push(one);
    for hex in [
        "e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800",
        "5f9c95bca3508c24b1d0b1559c83ef5b04445cc4581c8e86d8224eddd09f1157",
    ] {
        points.push(
            data_encoding::HEXLOWER
                .decode(hex.as_bytes())
                .unwrap()
                .try_into()
                .unwrap(),
        );
    }
    for first in [0xec, 0xed, 0xee] {
        let mut point = [0xff; 32];
        point[0] = first;
        point[31] = 0x7f;
        points.push(point);
    }
    for point in points {
        for high_bit in [0, 0x80] {
            let mut alias = point;
            alias[31] |= high_bit;
            assert_eq!(
                encryption::seal(b"synthetic", &alias),
                Err(Error::InvalidRecipient)
            );
            assert_eq!(encryption::seal(b"", &alias), Err(Error::InvalidRecipient));
        }
    }
}

#[test]
fn accepted_recipient_encoding_is_not_normalized() {
    let (mut public, secret) = keypair();
    public.0[31] |= 0x80;
    let ciphertext = encryption::seal(b"synthetic", &public.0).unwrap();
    assert_eq!(
        sealedbox::open(&ciphertext, &public, &secret).unwrap(),
        b"synthetic"
    );
}
