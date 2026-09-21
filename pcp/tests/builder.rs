use std::time::UNIX_EPOCH;

use orb_pcp::{
    self as pcp, BackendKey, BackendKeys, BiometricPolicy, BuildError, BuildRequest,
    MetadataError, PackageInfo, PcpVersion,
};
use rand::{CryptoRng, RngCore};
use sodiumoxide::crypto::box_;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("synthetic signing failure")]
struct SignerError;

fn request(key: &[u8; 32]) -> BuildRequest<'_> {
    let backend = || BackendKey {
        public_key: key,
        encrypted_private_key: "synthetic-envelope",
    };
    BuildRequest {
        version: PcpVersion::V2_8,
        timestamp: 1,
        info: PackageInfo {
            signup_id: "synthetic",
            signup_reason: "test",
            orb_id: "orb",
            operator_id: "operator",
            capture_start: UNIX_EPOCH,
            qr_code: "qr",
            id_commitment: "id",
            software_version: "test",
            orb_country: "country",
            orb_public_key_certificate: b"synthetic-certificate",
            device_public_key: Some("device"),
        },
        user_public_key: key,
        backend_keys: BackendKeys {
            iris: backend(),
            normalized_iris: backend(),
            face: backend(),
            tier2: backend(),
        },
        biometrics: BiometricPolicy::Redacted,
    }
}

struct FailingRng;
impl CryptoRng for FailingRng {}
impl RngCore for FailingRng {
    fn next_u32(&mut self) -> u32 {
        panic!("unexpected randomness")
    }
    fn next_u64(&mut self) -> u64 {
        panic!("unexpected randomness")
    }
    fn fill_bytes(&mut self, _: &mut [u8]) {
        panic!("expected fallible randomness")
    }
    fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand::Error> {
        Err(rand::Error::new(std::io::Error::other(
            "synthetic entropy failure",
        )))
    }
}

#[test]
fn version_device_binding_mismatches_fail_before_signing() {
    for version in [PcpVersion::V2_7, PcpVersion::V2_8] {
        let mut input = request(&[0; 32]);
        input.version = version;
        if version == PcpVersion::V2_8 {
            input.info.device_public_key = None;
        }
        let result = pcp::build(
            &input,
            &mut FailingRng,
            |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
        );
        assert!(matches!(result, Err(BuildError::DeviceKeyVersionMismatch)));
    }
}

#[test]
fn oversized_archive_timestamp_fails_before_signing() {
    let mut input = request(&[0; 32]);
    input.timestamp = u64::from(u32::MAX) + 1;
    let result = pcp::build(
        &input,
        &mut FailingRng,
        |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
    );
    assert!(matches!(
        result,
        Err(BuildError::Archive(pcp::ArchiveError::TimestampOutOfRange))
    ));
}

#[test]
fn randomness_failure_returns_no_package_or_signature() {
    sodiumoxide::init().unwrap();
    let (key, _) = box_::gen_keypair();
    let result = pcp::build(
        &request(&key.0),
        &mut FailingRng,
        |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
    );
    assert!(matches!(
        result,
        Err(BuildError::Metadata(MetadataError::Randomness(_)))
    ));
}

#[test]
fn invalid_user_recipient_returns_no_package_or_signature() {
    let result = pcp::build(
        &request(&[0; 32]),
        &mut rand::rngs::OsRng,
        |_| -> Result<Vec<u8>, SignerError> { panic!("must not sign") },
    );
    assert!(matches!(
        result,
        Err(BuildError::Encryption(pcp::SealingError::InvalidRecipient))
    ));
}

#[test]
fn signer_failure_is_not_retried_or_returned_as_a_package() {
    sodiumoxide::init().unwrap();
    let (key, _) = box_::gen_keypair();
    let mut calls = 0;
    let result = pcp::build(&request(&key.0), &mut rand::rngs::OsRng, |_| {
        calls += 1;
        Err::<Vec<u8>, _>(SignerError)
    });
    assert_eq!(calls, 1);
    assert!(matches!(
        result,
        Err(BuildError::Signing(pcp::SigningError::Signer(SignerError)))
    ));
}
