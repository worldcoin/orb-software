use std::io::Read;

use orb_pcp::layout::{self, BiometricArchives, Biometrics, Format, Tier0Files};

fn biometrics(fraud: bool) -> Biometrics<'static> {
    Biometrics {
        archives: BiometricArchives {
            iris_sealed: b"iris-ciphertext",
            normalized_iris_sealed: b"normalized-ciphertext",
            face_sealed: b"face-ciphertext",
            fraud_sealed: fraud.then_some(b"fraud-ciphertext".as_slice()),
            face_ir_and_thermal: b"modality-archive",
        },
        face_embeddings_json: b"face-json",
        iris_codes_json: b"iris-json",
        iris_code_shares_json: [b"iris-share-0", b"iris-share-1", b"iris-share-2"],
        di_iris_embeddings_pb: b"di-protobuf",
        di_iris_embeddings_shares_pb: [b"di-share-0", b"di-share-1", b"di-share-2"],
    }
}

fn common_files() -> Tier0Files<'static> {
    Tier0Files {
        info_json: b"info-json",
        hashes_json: b"hashes-json",
        hashes_signature: b"signature",
        backend_keys_json: b"backend-keys-json",
    }
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    tar::Archive::new(bytes)
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            assert_eq!(entry.header().mtime().unwrap(), 123);
            let name = entry.path().unwrap().to_str().unwrap().to_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}

fn assert_entries(actual: &[u8], expected: &[(&str, &[u8])]) {
    let expected: Vec<_> = expected
        .iter()
        .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
        .collect();
    assert_eq!(entries(actual), expected);
}

#[test]
fn v2_places_all_archives_in_tier0_before_payloads() {
    for fraud in [false, true] {
        let bio = biometrics(fraud);
        let auxiliary = layout::auxiliary_tiers(Format::V2, 123, Some(&bio)).unwrap();
        assert_eq!(auxiliary.tier1, vec![0; 1024]);
        assert_eq!(auxiliary.tier2, vec![0; 1024]);
        let bytes = layout::tier0(Format::V2, 123, common_files(), Some(&bio)).unwrap();
        let mut expected: Vec<(&str, &[u8])> = vec![
            ("iris.tar", b"iris-ciphertext"),
            ("normalized_iris.tar", b"normalized-ciphertext"),
            ("face.tar", b"face-ciphertext"),
        ];
        if fraud {
            expected.push(("fraud.tar", b"fraud-ciphertext"));
        }
        expected.push(("face_ir_and_thermal.tar", b"modality-archive"));
        expected.extend(full_tier0_remainder());
        assert_entries(&bytes, &expected);
    }
}

#[test]
fn v3_routes_archives_to_auxiliary_tiers_without_changing_bytes() {
    for fraud in [false, true] {
        let bio = biometrics(fraud);
        let auxiliary = layout::auxiliary_tiers(Format::V3, 123, Some(&bio)).unwrap();
        let mut expected: Vec<(&str, &[u8])> = vec![
            ("iris.tar", b"iris-ciphertext"),
            ("normalized_iris.tar", b"normalized-ciphertext"),
            ("face.tar", b"face-ciphertext"),
        ];
        if fraud {
            expected.push(("fraud.tar", b"fraud-ciphertext"));
        }
        assert_entries(&auxiliary.tier1, &expected);
        assert_entries(
            &auxiliary.tier2,
            &[("face_ir_and_thermal.tar", b"modality-archive")],
        );
        let tier0 = layout::tier0(Format::V3, 123, common_files(), Some(&bio)).unwrap();
        assert_entries(&tier0, &full_tier0_remainder());
    }
}

#[test]
fn omitted_biometrics_leave_only_four_tier0_files_and_empty_auxiliary_tiers() {
    for format in [Format::V2, Format::V3] {
        let auxiliary = layout::auxiliary_tiers(format, 123, None).unwrap();
        assert_eq!(auxiliary.tier1, vec![0; 1024]);
        assert_eq!(auxiliary.tier2, vec![0; 1024]);
        let tier0 = layout::tier0(format, 123, common_files(), None).unwrap();
        assert_entries(
            &tier0,
            &[
                ("info.json", b"info-json"),
                ("hashes.sign", b"signature"),
                ("hashes.json", b"hashes-json"),
                ("backend_keys.json", b"backend-keys-json"),
            ],
        );
    }
}

#[test]
fn empty_di_and_share_payloads_are_present_not_omitted() {
    for format in [Format::V2, Format::V3] {
        let mut bio = biometrics(false);
        bio.di_iris_embeddings_pb = b"";
        bio.di_iris_embeddings_shares_pb = [b""; 3];
        let tier0 = layout::tier0(format, 123, common_files(), Some(&bio)).unwrap();
        let entries = entries(&tier0);
        let di_files: Vec<_> = entries
            .iter()
            .filter(|(name, _)| name.starts_with("di_iris_embeddings"))
            .collect();
        assert_eq!(di_files.len(), 4);
        assert!(di_files.iter().all(|(_, bytes)| bytes.is_empty()));
    }
}

fn full_tier0_remainder() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("info.json", b"info-json"),
        ("face_embeddings.json", b"face-json"),
        ("iris_codes.json", b"iris-json"),
        ("iris_code_shares_0.json", b"iris-share-0"),
        ("iris_code_shares_1.json", b"iris-share-1"),
        ("iris_code_shares_2.json", b"iris-share-2"),
        ("di_iris_embeddings.pb", b"di-protobuf"),
        ("di_iris_embeddings_shares_0.pb", b"di-share-0"),
        ("di_iris_embeddings_shares_1.pb", b"di-share-1"),
        ("di_iris_embeddings_shares_2.pb", b"di-share-2"),
        ("hashes.sign", b"signature"),
        ("hashes.json", b"hashes-json"),
        ("backend_keys.json", b"backend-keys-json"),
    ]
}
