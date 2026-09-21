use orb_pcp::manifest::{self, Error, Version};

#[test]
fn v2_formats_have_exact_sorted_compact_bytes() {
    for (version, wire_version) in [(Version::V2_7, "2.7"), (Version::V2_8, "2.8")] {
        let actual =
            manifest::encode(version, [("z.bin", [0xff; 32]), ("a.bin", [0x01; 32])])
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
        Version::V3_0 {
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
    let actual = manifest::encode(Version::V2_8, [("a\"\\\nλ", [0; 32])]).unwrap();
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
        manifest::encode(Version::V2_7, []).unwrap(),
        br#"{"version":"2.7"}"#
    );
    assert_eq!(
        manifest::encode(Version::V2_8, []).unwrap(),
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
                Err(Error::DuplicateEntry)
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
                Err(Error::ReservedEntry)
            ));
        }
    }
}

fn versions() -> [Version; 3] {
    [
        Version::V2_7,
        Version::V2_8,
        Version::V3_0 {
            tier_1: [0xab; 32],
            tier_2: [0xcd; 32],
        },
    ]
}
