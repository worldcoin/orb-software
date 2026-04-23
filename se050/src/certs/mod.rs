mod null_params_alg;

use std::sync::OnceLock;

use p256::pkcs8::spki::{DecodePublicKey, Error as SpkiError};
use rustls_pki_types::{
    pem::{Error as PemError, PemObject as _},
    CertificateDer, TrustAnchor, UnixTime,
};
use webpki::{EndEntityCert, KeyUsage};

use self::null_params_alg::NXP_VERIFICATION_ALGS;

/// Verifies the chip-unique certificate `chip_cert_pem`, and extracts its P-256
/// public key.
pub fn verify_cert(
    chip_cert_pem: &str,
    time: UnixTime,
) -> Result<p256::PublicKey, VerifyCertErr> {
    verify_cert_inner(chip_cert_pem, time).map_err(VerifyCertErr::from)
}

/// Opaque error for verifying certs.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct VerifyCertErr(#[from] VerifyCertInnerErr);

/// From AN12436, section 3.11.3, Root attestation cert
/// (Subject OU=Plug and Trust, O=NXP, CN=NXP RootCAvE506)
fn root_nxp_cert() -> &'static TrustAnchor<'static> {
    const NXP_ROOT_CERT: CertificateDer<'static> = CertificateDer::from_slice(
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/63709315050002.crt")),
    );
    static ONCE: OnceLock<TrustAnchor<'static>> = OnceLock::new();

    ONCE.get_or_init(|| {
        webpki::anchor_from_trusted_cert(&NXP_ROOT_CERT)
            .expect("known good cert should always work")
            .to_owned()
    })
}

/// From AN12436, section 3.11.3, Intermediate attestation cert
/// (Subject OU=Plug and Trust, O=NXP, CN=NXP Intermediate-AttestationCAvE206)
const NXP_INTERMEDIATE_CERT: CertificateDer<'static> = CertificateDer::from_slice(
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/63709315060003.crt")),
);

#[derive(Debug, thiserror::Error)]
enum ParsePemErr {
    #[error("invalid PEM file: {0}")]
    InvalidPem(#[from] PemError),
    #[error("empty PEM")]
    Empty,
    #[error("the pem file contains more than one cert")]
    MoreThanOneCertificate,
}

fn parse_pem_cert(pem: &str) -> Result<CertificateDer<'_>, ParsePemErr> {
    let mut items = CertificateDer::pem_slice_iter(pem.as_bytes());
    let cert = items.next().ok_or(ParsePemErr::Empty)??;
    if items.next().is_some() {
        return Err(ParsePemErr::MoreThanOneCertificate);
    }

    Ok(cert)
}

#[derive(Debug, thiserror::Error)]
enum VerifyCertInnerErr {
    #[error(transparent)]
    ParsePem(#[from] ParsePemErr),
    #[error("invalid end entity cert: {0}")]
    InvalidEndEntityCert(#[source] webpki::Error),
    #[error("error while verifying for usage: {0}")]
    VerifyForUsageErr(#[source] webpki::Error),
    #[error("failed to convert from SPKI to P-256 pubkey: {0}")]
    SpkiError(#[from] SpkiError),
}

fn verify_cert_inner(
    chip_cert_pem: &str,
    time: UnixTime,
) -> Result<p256::PublicKey, VerifyCertInnerErr> {
    let der_cert = parse_pem_cert(chip_cert_pem)?;
    let end_entity_cert: EndEntityCert = EndEntityCert::try_from(&der_cert)
        .map_err(VerifyCertInnerErr::InvalidEndEntityCert)?;

    let key_usage = KeyUsage::client_auth(); // TODO: Is this correct?
    let _verified_path = end_entity_cert
        .verify_for_usage(
            NXP_VERIFICATION_ALGS,
            &[root_nxp_cert().clone()],
            &[NXP_INTERMEDIATE_CERT],
            time,
            key_usage,
            None,
            None,
        )
        .map_err(VerifyCertInnerErr::VerifyForUsageErr)?;

    let spki = end_entity_cert.subject_public_key_info();
    let pubkey = p256::PublicKey::from_public_key_der(spki.as_ref())?;

    Ok(pubkey)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::example_data::CERT;

    use std::time::Duration;

    // Generated with openssl x509 -in 2A66F1B2.crt -pubkey -noout
    const EXPECTED_PUBKEY: &str = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEWCtEfBCALWbxjT8pwuwXcjg8UULO
ZqFsXAd6a0FUgQwafxI+5wkqRJ4I7QFvbmPxtCRRUoJ7QPmX+DkUqWwrfw==
-----END PUBLIC KEY-----"#;

    // NOTE: All time ranges are valid from [start, end], inclusive.

    const APR_25_2019_13_58_03: Duration = Duration::from_secs(1556200683);
    const APR_25_2037_13_58_03: Duration = Duration::from_secs(2124280683);
    const ROOT_RANGE: (Duration, Duration) =
        (APR_25_2019_13_58_03, APR_25_2037_13_58_03);

    const APR_25_2019_14_10_21: Duration = Duration::from_secs(1556201421);
    const APR_25_2034_14_10_21: Duration = Duration::from_secs(2029587021);
    const INTERMEDIATE_RANGE: (Duration, Duration) =
        (APR_25_2019_14_10_21, APR_25_2034_14_10_21);

    const DEC_1_2023: Duration = Duration::from_secs(1701388800);
    const NOV_28_2035: Duration = Duration::from_secs(2079820800);
    const END_ENTITY_RANGE: (Duration, Duration) = (DEC_1_2023, NOV_28_2035);

    fn compute_total_range() -> (Duration, Duration) {
        let start = ROOT_RANGE
            .0
            .max(INTERMEDIATE_RANGE.0)
            .max(END_ENTITY_RANGE.0);
        let end = ROOT_RANGE
            .1
            .min(INTERMEDIATE_RANGE.1)
            .min(END_ENTITY_RANGE.1);

        (start, end)
    }

    #[test]
    fn test_known_cert_not_expired() {
        let expected_pubkey = p256::PublicKey::from_public_key_pem(EXPECTED_PUBKEY)
            .expect("known good pubkey should always work");

        let (start, end) = compute_total_range();
        for time in [
            start,
            start + Duration::from_secs(1),
            end - Duration::from_secs(1),
            end,
            END_ENTITY_RANGE.0,
            INTERMEDIATE_RANGE.1,
        ] {
            let time = UnixTime::since_unix_epoch(time);
            let pubkey = verify_cert(CERT, time)
                .unwrap_or_else(|_| panic!("cert should be valid at {time:?}"));
            assert_eq!(pubkey, expected_pubkey);
        }
    }

    #[test]
    fn test_known_cert_invalid_after() {
        for time in [
            ROOT_RANGE.1,
            INTERMEDIATE_RANGE.1 + Duration::from_secs(1),
            END_ENTITY_RANGE.1,
        ] {
            let time = UnixTime::since_unix_epoch(time);
            let err_msg = format!("cert should have expired by {time:?}");
            let err = verify_cert(CERT, time).expect_err(&err_msg);
            assert!(
                matches!(
                    err.0,
                    VerifyCertInnerErr::VerifyForUsageErr(
                        webpki::Error::CertExpired { .. }
                    )
                ),
                "{err_msg}"
            );
        }
    }

    #[test]
    fn test_known_cert_invalid_before() {
        for time in [
            ROOT_RANGE.0,
            INTERMEDIATE_RANGE.0,
            END_ENTITY_RANGE.0 - Duration::from_secs(1),
        ] {
            let time = UnixTime::since_unix_epoch(time);
            let err_msg = format!("cert should not be valid by {time:?}");
            let err = verify_cert(CERT, time).expect_err(&err_msg);
            assert!(
                matches!(
                    err.0,
                    VerifyCertInnerErr::VerifyForUsageErr(
                        webpki::Error::CertNotValidYet { .. }
                    )
                ),
                "{err_msg}"
            );
        }
    }
}
