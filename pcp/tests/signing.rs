use orb_pcp::manifest::{self, SigningError, Version};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("synthetic signer unavailable")]
struct SignerUnavailable;

#[test]
fn signer_receives_one_raw_digest_and_signature_bytes_are_unchanged() {
    let expected = data_encoding::HEXLOWER
        .decode(b"ea3623bce4f4d595e0f663a68d50f90c7d453d8593adea57ba740c80d96f81b9")
        .unwrap();
    let mut calls = 0;
    let signed = manifest::encode_and_sign(Version::V2_8, [], |digest| {
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
        Version::V2_7,
        Version::V2_8,
        Version::V3_0 {
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
            ring::digest::digest(&ring::digest::SHA256, &signed.hashes_json).as_ref()
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
            Version::V2_8,
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
    let result = manifest::encode_and_sign(Version::V2_8, [], |_| {
        calls += 1;
        Err::<Vec<u8>, _>(SignerUnavailable)
    });
    assert_eq!(calls, 1);
    assert!(matches!(
        result,
        Err(SigningError::Signer(SignerUnavailable))
    ));
}
