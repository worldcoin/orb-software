mod sealed_boxes {
    use crate::crypto::{self, SealingError};
    use alkali::{
        asymmetric::seal::curve25519xsalsa20poly1305 as sealedbox, AlkaliError,
    };

    fn open(
        ciphertext: &[u8],
        pair: &sealedbox::Keypair,
    ) -> Result<Vec<u8>, AlkaliError> {
        let mut plaintext =
            vec![0; ciphertext.len().saturating_sub(sealedbox::OVERHEAD_LENGTH)];
        sealedbox::decrypt(ciphertext, pair, &mut plaintext)?;
        Ok(plaintext)
    }

    #[test]
    fn encrypted_bytes_open_with_libsodium() {
        let pair = sealedbox::Keypair::generate().unwrap();
        for size in [0, 1, 256, 4096] {
            let plaintext: Vec<_> = (0_u8..=255).cycle().take(size).collect();
            let encrypted = crypto::seal(&plaintext, &pair.public_key).unwrap();
            assert_eq!(encrypted.len(), plaintext.len() + 48);
            assert_eq!(open(&encrypted, &pair).unwrap(), plaintext);
        }
    }

    #[test]
    fn repeated_encryption_uses_fresh_ephemeral_keys() {
        let pair = sealedbox::Keypair::generate().unwrap();
        let first = crypto::seal(b"synthetic", &pair.public_key).unwrap();
        let second = crypto::seal(b"synthetic", &pair.public_key).unwrap();
        assert_ne!(first, second);
        assert_ne!(&first[..32], &second[..32]);
        for ciphertext in [first, second] {
            assert_eq!(open(&ciphertext, &pair).unwrap(), b"synthetic");
        }
    }

    #[test]
    fn wrong_recipient_tampering_and_truncation_fail_to_open() {
        let pair = sealedbox::Keypair::generate().unwrap();
        let other = sealedbox::Keypair::generate().unwrap();
        let ciphertext = crypto::seal(b"synthetic", &pair.public_key).unwrap();
        assert!(open(&ciphertext, &other).is_err());
        for i in 0..ciphertext.len() {
            let mut changed = ciphertext.clone();
            changed[i] ^= 1;
            assert!(open(&changed, &pair).is_err());
        }
        for length in 0..ciphertext.len() {
            assert!(open(&ciphertext[..length], &pair).is_err());
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
                    crypto::seal(b"synthetic", &alias),
                    Err(SealingError::InvalidRecipient)
                );
                assert_eq!(
                    crypto::seal(b"", &alias),
                    Err(SealingError::InvalidRecipient)
                );
            }
        }
    }

    #[test]
    fn accepted_recipient_encoding_is_not_normalized() {
        let mut pair = sealedbox::Keypair::generate().unwrap();
        pair.public_key[31] |= 0x80;
        let ciphertext = crypto::seal(b"synthetic", &pair.public_key).unwrap();
        assert_eq!(open(&ciphertext, &pair).unwrap(), b"synthetic");
    }
}

mod hyrax {
    use crate::crypto::{self, CommitmentError};
    use hyrax::iriscode_commit::compute_commitments_binary_outputs;
    use rand::{CryptoRng, RngCore, SeedableRng};

    // Deterministic test double only; never a production entropy source.
    struct SeedRng {
        seed: [u8; 32],
        calls: usize,
        fail: bool,
    }

    impl CryptoRng for SeedRng {}

    impl RngCore for SeedRng {
        fn next_u32(&mut self) -> u32 {
            panic!("use fallible byte generation")
        }

        fn next_u64(&mut self) -> u64 {
            panic!("use fallible byte generation")
        }

        fn fill_bytes(&mut self, _: &mut [u8]) {
            panic!("use fallible byte generation")
        }

        fn try_fill_bytes(&mut self, bytes: &mut [u8]) -> Result<(), rand::Error> {
            assert_eq!(bytes.len(), 32);
            self.calls += 1;
            bytes.copy_from_slice(&self.seed);
            if self.fail {
                return Err(rand::Error::new(std::io::Error::other(
                    "synthetic entropy failure",
                )));
            }
            Ok(())
        }
    }

    #[test]
    fn exact_outputs_match_pinned_implementation_and_keep_original_data() {
        let seed = [0x5a; 32];
        for len in [257, 512, 513, 2048] {
            let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let expected = compute_commitments_binary_outputs(&data, seed);
            let mut rng = SeedRng {
                seed,
                calls: 0,
                fail: false,
            };
            let output = crypto::generate_commitment(&data, &mut rng).unwrap();
            assert_eq!(rng.calls, 1);
            assert_eq!(output.data(), data);
            assert_eq!(output.data().as_ptr(), data.as_ptr());
            assert_eq!(output.commitment(), expected.commitment_serialized);
            assert_eq!(
                output.blinding_factors(),
                expected.blinding_factors_serialized
            );
            assert!(!output.commitment().is_empty());
        }
    }

    #[test]
    fn small_inputs_preserve_legacy_empty_outputs() {
        for len in [0, 1, 255, 256] {
            let data = vec![0x42; len];
            let mut rng = rand::rngs::StdRng::seed_from_u64(42);
            let output = crypto::generate_commitment(&data, &mut rng).unwrap();
            assert_eq!(output.data(), data);
            assert!(output.commitment().is_empty());
            assert!(output.blinding_factors().is_empty());
        }
    }

    #[test]
    fn each_generation_draws_a_fresh_seed() {
        let data = [0x42; 512];
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let first = crypto::generate_commitment(&data, &mut rng).unwrap();
        let second = crypto::generate_commitment(&data, &mut rng).unwrap();
        assert_ne!(first.commitment(), second.commitment());
        assert_ne!(first.blinding_factors(), second.blinding_factors());
    }

    #[test]
    fn entropy_failure_returns_no_generated_record_and_is_not_retried() {
        let mut rng = SeedRng {
            seed: [0x5a; 32],
            calls: 0,
            fail: true,
        };
        let result = crypto::generate_commitment(&[0x42; 512], &mut rng);
        assert!(matches!(result, Err(CommitmentError::Randomness(_))));
        assert_eq!(rng.calls, 1);
    }
}
