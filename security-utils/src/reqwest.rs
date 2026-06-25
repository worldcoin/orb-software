use std::sync::OnceLock;

use eyre::{Result, WrapErr};
use reqwest::{Client, ClientBuilder};

pub use reqwest;
use reqwest::Certificate;

use crate::certs::{
    verify_pinned_cert, AWS_ROOT_CA1_CERT, AWS_ROOT_CA1_SHA256, AWS_ROOT_CA2_CERT,
    AWS_ROOT_CA2_SHA256, AWS_ROOT_CA3_CERT, AWS_ROOT_CA3_SHA256, AWS_ROOT_CA4_CERT,
    AWS_ROOT_CA4_SHA256, GTS_ROOT_R1_CERT, GTS_ROOT_R1_SHA256, GTS_ROOT_R2_CERT,
    GTS_ROOT_R2_SHA256, GTS_ROOT_R3_CERT, GTS_ROOT_R3_SHA256, GTS_ROOT_R4_CERT,
    GTS_ROOT_R4_SHA256, SFS_ROOT_G2_CERT, SFS_ROOT_G2_SHA256,
};

/// Important certificates we vendor for security
#[derive(Debug)]
pub struct VendoredCerts {
    /// AWS Root CA
    pub aws_root_ca1: Certificate,
    pub aws_root_ca2: Certificate,
    pub aws_root_ca3: Certificate,
    pub aws_root_ca4: Certificate,
    pub sfs_root_g2: Certificate,
    /// Google Trust Services Root CAs
    pub gts_root_r1: Certificate,
    pub gts_root_r2: Certificate,
    pub gts_root_r3: Certificate,
    pub gts_root_r4: Certificate,
}

pub fn get_certs() -> &'static VendoredCerts {
    static CERTS: OnceLock<VendoredCerts> = OnceLock::new();
    CERTS.get_or_init(|| {
        let aws_root_ca1 = make_cert(AWS_ROOT_CA1_CERT, &AWS_ROOT_CA1_SHA256)
            .expect("Failed to make AWS CA1 cert");
        let aws_root_ca2 = make_cert(AWS_ROOT_CA2_CERT, &AWS_ROOT_CA2_SHA256)
            .expect("Failed to make AWS CA2 cert");
        let aws_root_ca3 = make_cert(AWS_ROOT_CA3_CERT, &AWS_ROOT_CA3_SHA256)
            .expect("Failed to make AWS CA3 cert");
        let aws_root_ca4 = make_cert(AWS_ROOT_CA4_CERT, &AWS_ROOT_CA4_SHA256)
            .expect("Failed to make AWS CA4 cert");
        let sfs_root_g2 = make_cert(SFS_ROOT_G2_CERT, &SFS_ROOT_G2_SHA256)
            .expect("Failed to make SFS G2 cert");

        let gts_root_r1 = make_cert(GTS_ROOT_R1_CERT, &GTS_ROOT_R1_SHA256)
            .expect("Failed to make GTS R1 cert");
        let gts_root_r2 = make_cert(GTS_ROOT_R2_CERT, &GTS_ROOT_R2_SHA256)
            .expect("Failed to make GTS R2 cert");
        let gts_root_r3 = make_cert(GTS_ROOT_R3_CERT, &GTS_ROOT_R3_SHA256)
            .expect("Failed to make GTS R3 cert");
        let gts_root_r4 = make_cert(GTS_ROOT_R4_CERT, &GTS_ROOT_R4_SHA256)
            .expect("Failed to make GTS R4 cert");

        VendoredCerts {
            aws_root_ca1,
            aws_root_ca2,
            aws_root_ca3,
            aws_root_ca4,
            sfs_root_g2,
            gts_root_r1,
            gts_root_r2,
            gts_root_r3,
            gts_root_r4,
        }
    })
}

fn make_cert(cert_pem: &[u8], sha256: &[u8; 32]) -> Result<Certificate> {
    verify_pinned_cert(cert_pem, sha256)?;
    Certificate::from_pem(cert_pem).wrap_err("certificate failed to parse")
}

/// Used to de-duplicate boilerplate in http client builder.
macro_rules! helper {
    ($builder:expr, $certs:expr) => {{
        let certs = $certs;
        $builder
            .min_tls_version(reqwest::tls::Version::TLS_1_3)
            .tls_built_in_root_certs(false)
            .https_only(!cfg!(feature = "dangerously-allow-http"))
            .add_root_certificate(certs.aws_root_ca1.clone())
            .add_root_certificate(certs.aws_root_ca2.clone())
            .add_root_certificate(certs.aws_root_ca3.clone())
            .add_root_certificate(certs.aws_root_ca4.clone())
            .add_root_certificate(certs.sfs_root_g2.clone())
            .add_root_certificate(certs.gts_root_r1.clone())
            .add_root_certificate(certs.gts_root_r2.clone())
            .add_root_certificate(certs.gts_root_r3.clone())
            .add_root_certificate(certs.gts_root_r4.clone())
            .redirect(reqwest::redirect::Policy::none())
    }};
}

#[allow(clippy::disallowed_methods)]
pub fn client_builder() -> ClientBuilder {
    let certs = get_certs();
    helper!(Client::builder(), certs)
}

#[cfg(feature = "blocking")]
pub mod blocking {
    use reqwest::blocking::{Client, ClientBuilder};

    use super::get_certs;

    #[allow(clippy::disallowed_methods)]
    pub fn client_builder() -> ClientBuilder {
        let certs = get_certs();
        helper!(Client::builder(), certs)
    }
}
