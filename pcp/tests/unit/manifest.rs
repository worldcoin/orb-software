mod encoding {
    use crate::manifest::{self, ManifestError, ManifestFormat};

    #[test]
    fn v2_formats_have_exact_sorted_compact_bytes() {
        for (version, wire_version) in
            [(ManifestFormat::V2_7, "2.7"), (ManifestFormat::V2_8, "2.8")]
        {
            let actual = manifest::encode(
                version,
                [("z.bin", [0xff; 32]), ("a.bin", [0x01; 32])],
            )
            .unwrap();
            let expected = format!(
                concat!(
                    "{{\"a.bin\":\"0101010101010101010101010101010101010101010101010101010101010101\",",
                    "\"version\":\"{}\",",
                    "\"z.bin\":\"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"}}"
                ),
                wire_version
            );
            assert_eq!(actual, expected.as_bytes());
        }
    }

    #[test]
    fn v3_includes_encrypted_tier_digests_and_absent_tier_sentinels() {
        let actual = manifest::encode(
            ManifestFormat::V3_0 {
                tier_1: [0xab; 32],
                tier_2: [0xcd; 32],
            },
            [("a.bin", [0x01; 32])],
        )
        .unwrap();
        let expected = concat!(
            "{\"a.bin\":\"0101010101010101010101010101010101010101010101010101010101010101\",",
            "\"tier_1\":\"abababababababababababababababababababababababababababababababab\",",
            "\"tier_2\":\"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd\",",
            "\"tier_3\":\"0000000000000000000000000000000000000000000000000000000000000000\",",
            "\"tier_4\":\"0000000000000000000000000000000000000000000000000000000000000000\",",
            "\"tier_5\":\"0000000000000000000000000000000000000000000000000000000000000000\",",
            "\"version\":\"3.0\"}"
        );
        assert_eq!(actual, expected.as_bytes());
    }

    #[test]
    fn input_order_does_not_change_signed_bytes() {
        let entries = [("z", [0x01; 32]), ("a", [0xab; 32]), ("m", [0xff; 32])];
        for version in versions() {
            assert_eq!(
                manifest::encode(version, entries).unwrap(),
                manifest::encode(version, entries.into_iter().rev()).unwrap()
            );
        }
    }

    #[test]
    fn names_are_json_escaped_without_changing_utf8() {
        let actual =
            manifest::encode(ManifestFormat::V2_8, [("a\"\\\nλ", [0; 32])]).unwrap();
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
    fn empty_payloads_still_encode_the_format_fields() {
        assert_eq!(
            manifest::encode(ManifestFormat::V2_7, []).unwrap(),
            br#"{"version":"2.7"}"#
        );
        assert_eq!(
            manifest::encode(ManifestFormat::V2_8, []).unwrap(),
            br#"{"version":"2.8"}"#
        );
        let bytes = manifest::encode(versions()[2], []).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 6);
        assert_eq!(value["version"], "3.0");
    }

    #[test]
    fn duplicate_entries_are_rejected_even_when_digests_match() {
        for version in versions() {
            for second in [[0x01; 32], [0xff; 32]] {
                assert!(matches!(
                    manifest::encode(version, [("a", [0x01; 32]), ("a", second)]),
                    Err(ManifestError::DuplicateEntry)
                ));
            }
        }
    }

    #[test]
    fn callers_cannot_override_reserved_fields() {
        for version in versions() {
            for name in ["version", "tier_1", "tier_2", "tier_3", "tier_4", "tier_5"] {
                assert!(matches!(
                    manifest::encode(version, [(name, [0xff; 32])]),
                    Err(ManifestError::ReservedEntry)
                ));
            }
        }
    }

    fn versions() -> [ManifestFormat; 3] {
        [
            ManifestFormat::V2_7,
            ManifestFormat::V2_8,
            ManifestFormat::V3_0 {
                tier_1: [0xab; 32],
                tier_2: [0xcd; 32],
            },
        ]
    }
}

mod signing {
    use crate::manifest::{self, ManifestFormat, SigningError};

    #[derive(Debug, PartialEq, Eq, thiserror::Error)]
    #[error("synthetic signer unavailable")]
    struct SignerUnavailable;

    #[test]
    fn signer_receives_one_raw_digest_and_signature_bytes_are_unchanged() {
        let expected = data_encoding::HEXLOWER
            .decode(b"ea3623bce4f4d595e0f663a68d50f90c7d453d8593adea57ba740c80d96f81b9")
            .unwrap();
        let mut calls = 0;
        let signed = manifest::encode_and_sign(ManifestFormat::V2_8, [], |digest| {
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
    fn digest_covers_exact_returned_json_for_every_version() {
        for version in [
            ManifestFormat::V2_7,
            ManifestFormat::V2_8,
            ManifestFormat::V3_0 {
                tier_1: [1; 32],
                tier_2: [2; 32],
            },
        ] {
            let entries = [("z", [3; 32]), ("a\"\\\nλ", [4; 32])];
            let mut captured = None;
            let signed = manifest::encode_and_sign(version, entries, |digest| {
                captured = Some(*digest);
                Ok::<_, SignerUnavailable>(b"synthetic-signature".to_vec())
            })
            .unwrap();
            assert_eq!(
                signed.hashes_json,
                manifest::encode(version, entries).unwrap()
            );
            assert_eq!(
                captured.unwrap().as_slice(),
                ring::digest::digest(&ring::digest::SHA256, &signed.hashes_json)
                    .as_ref()
            );
        }
    }

    #[test]
    fn malformed_manifest_never_calls_the_signer() {
        for entries in [
            vec![("version", [0; 32])],
            vec![("same", [0; 32]), ("same", [1; 32])],
        ] {
            let result = manifest::encode_and_sign(
                ManifestFormat::V2_8,
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
        let result = manifest::encode_and_sign(ManifestFormat::V2_8, [], |_| {
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
