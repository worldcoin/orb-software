mod encoding {
    use crate::manifest::{self, ManifestError};

    #[test]
    fn manifest_has_exact_sorted_compact_bytes() {
        let actual =
            manifest::encode([("z.bin", [0xff; 32]), ("a.bin", [0x01; 32])]).unwrap();
        let expected = concat!(
            "{\"a.bin\":\"0101010101010101010101010101010101010101010101010101010101010101\",",
            "\"version\":\"2.8\",",
            "\"z.bin\":\"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"}"
        );
        assert_eq!(actual, expected.as_bytes());
    }

    #[test]
    fn input_order_does_not_change_signed_bytes() {
        let entries = [("z", [0x01; 32]), ("a", [0xab; 32]), ("m", [0xff; 32])];
        assert_eq!(
            manifest::encode(entries).unwrap(),
            manifest::encode(entries.into_iter().rev()).unwrap()
        );
    }

    #[test]
    fn names_are_json_escaped_without_changing_utf8() {
        let actual = manifest::encode([("a\"\\\nλ", [0; 32])]).unwrap();
        assert_eq!(
            actual,
            concat!(
                "{\"a\\\"\\\\\\nλ\":",
                "\"0000000000000000000000000000000000000000000000000000000000000000\",",
                "\"version\":\"2.8\"}"
            )
            .as_bytes()
        );
    }

    #[test]
    fn empty_payloads_still_encode_the_version() {
        assert_eq!(manifest::encode([]).unwrap(), br#"{"version":"2.8"}"#);
    }

    #[test]
    fn duplicate_entries_are_rejected_even_when_digests_match() {
        for second in [[0x01; 32], [0xff; 32]] {
            assert!(matches!(
                manifest::encode([("a", [0x01; 32]), ("a", second)]),
                Err(ManifestError::DuplicateEntry)
            ));
        }
    }

    #[test]
    fn callers_cannot_override_the_version() {
        assert!(matches!(
            manifest::encode([("version", [0xff; 32])]),
            Err(ManifestError::ReservedEntry)
        ));
    }
}

mod signing {
    use crate::manifest::{self, SigningError};

    #[derive(Debug, PartialEq, Eq, thiserror::Error)]
    #[error("synthetic signer unavailable")]
    struct SignerUnavailable;

    #[test]
    fn signer_receives_one_raw_digest_and_signature_bytes_are_unchanged() {
        let expected = data_encoding::HEXLOWER
            .decode(b"ea3623bce4f4d595e0f663a68d50f90c7d453d8593adea57ba740c80d96f81b9")
            .unwrap();
        let mut calls = 0;
        let signed = manifest::encode_and_sign([], |digest| {
            calls += 1;
            assert_eq!(digest.as_slice(), expected);
            Ok::<_, SignerUnavailable>(vec![0, 255, 128, 10])
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(signed.hashes_json, br#"{"version":"2.8"}"#);
        assert_eq!(signed.hashes_signature, [0, 255, 128, 10]);
    }

    #[test]
    fn digest_covers_exact_returned_json() {
        let entries = [("z", [3; 32]), ("a\"\\\nλ", [4; 32])];
        let mut captured = None;
        let signed = manifest::encode_and_sign(entries, |digest| {
            captured = Some(*digest);
            Ok::<_, SignerUnavailable>(b"synthetic-signature".to_vec())
        })
        .unwrap();
        assert_eq!(signed.hashes_json, manifest::encode(entries).unwrap());
        assert_eq!(
            captured.unwrap().as_slice(),
            ring::digest::digest(&ring::digest::SHA256, &signed.hashes_json).as_ref()
        );
    }

    #[test]
    fn malformed_manifest_never_calls_the_signer() {
        for entries in [
            vec![("version", [0; 32])],
            vec![("same", [0; 32]), ("same", [1; 32])],
        ] {
            let result = manifest::encode_and_sign(
                entries,
                |_| -> Result<Vec<u8>, SignerUnavailable> {
                    panic!("signer must not be invoked on manifest error")
                },
            );
            assert!(matches!(result, Err(SigningError::Manifest(_))));
        }
    }

    #[test]
    fn signer_failure_is_preserved_without_retry() {
        let mut calls = 0;
        let result = manifest::encode_and_sign([], |_| {
            calls += 1;
            Err::<Vec<u8>, _>(SignerUnavailable)
        });
        assert_eq!(calls, 1);
        assert!(matches!(
            result,
            Err(SigningError::Signer(SignerUnavailable))
        ));
    }
}
