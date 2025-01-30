use std::sync::OnceLock;

use eyre::{ensure, Result, WrapErr};
use hex_literal::hex;

use reqwest::{Client, ClientBuilder};

pub use reqwest;
use reqwest::Certificate;

//
//  Amazon Trust Services - https://www.amazontrust.com/repository/
//  Updated by @oldgalileo (17/07/2024)
//
pub static AWS_ROOT_CA1_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/AmazonRootCA1.pem"
));
pub static AWS_ROOT_CA1_SHA256: [u8; 32] =
    hex!("2c43952ee9e000ff2acc4e2ed0897c0a72ad5fa72c3d934e81741cbd54f05bd1");

pub static AWS_ROOT_CA2_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/AmazonRootCA2.pem"
));
pub static AWS_ROOT_CA2_SHA256: [u8; 32] =
    hex!("a3a7fe25439d9a9b50f60af43684444d798a4c869305bf615881e5c84a44c1a2");

pub static AWS_ROOT_CA3_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/AmazonRootCA3.pem"
));
pub static AWS_ROOT_CA3_SHA256: [u8; 32] =
    hex!("3eb7c3258f4af9222033dc1bb3dd2c7cfa0982b98e39fb8e9dc095cfeb38126c");

pub static AWS_ROOT_CA4_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/AmazonRootCA4.pem"
));
pub static AWS_ROOT_CA4_SHA256: [u8; 32] =
    hex!("b0b7961120481e33670315b2f843e643c42f693c7a1010eb9555e06ddc730214");

// Starfield Root CA G2 certificate (acquired by Amazon)
pub static SFS_ROOT_G2_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/SFSRootCAG2.pem"
));
pub static SFS_ROOT_G2_SHA256: [u8; 32] =
    hex!("870f56d009d8aeb95b716b0e7b0020225d542c4b283b9ed896edf97428d6712e");

//
//  Google Trust Services - https://pki.goog/
//  Updated by @oldgalileo (16/07/2024)
//
pub static GTS_ROOT_R1_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/GTS_Root_R1.pem"
));
pub static GTS_ROOT_R1_SHA256: [u8; 32] =
    hex!("4195ea007a7ef8d3e2d338e8d9ff0083198e36bfa025442ddf41bb5213904fc2");

pub static GTS_ROOT_R2_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/GTS_Root_R2.pem"
));
pub static GTS_ROOT_R2_SHA256: [u8; 32] =
    hex!("1a49076630e489e4b1056804fb6c768397a9de52b236609aaf6ec5b94ce508ec");

pub static GTS_ROOT_R3_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/GTS_Root_R3.pem"
));
pub static GTS_ROOT_R3_SHA256: [u8; 32] =
    hex!("39238e09bb7d30e39fbf87746ceac206f7ec206cff3d73c743e3f818ca2ec54f");

pub static GTS_ROOT_R4_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/GTS_Root_R4.pem"
));
pub static GTS_ROOT_R4_SHA256: [u8; 32] =
    hex!("7e8b80d078d3dd77d3ed2108dd2b33412c12d7d72cb0965741c70708691776a2");

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

pub fn http_client_builder() -> ClientBuilder {
    let certs = get_certs();
    Client::builder()
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .tls_built_in_root_certs(false)
        .https_only(true)
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
}

fn make_cert(cert_pem: &[u8], sha256: &[u8; 32]) -> Result<Certificate> {
    // Verify that the certificate has not been replaced
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    context.update(cert_pem);
    let digest = context.finish();
    ensure!(
        digest.as_ref() == sha256,
        "provided cert bytes did not match sha256 hash"
    );

    Certificate::from_pem(cert_pem).wrap_err("certificate failed to parse")
}

#[cfg(feature = "blocking")]
pub mod blocking {
    use reqwest::blocking::{Client, ClientBuilder};

    use super::get_certs;

    pub fn http_client_builder() -> ClientBuilder {
        let certs = get_certs();
        Client::builder()
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .tls_built_in_root_certs(false)
            .https_only(true)
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_certs() {
        make_cert(AWS_ROOT_CA1_CERT, &AWS_ROOT_CA1_SHA256)
            .expect("Failed to make AWS CA1 cert");
        make_cert(AWS_ROOT_CA2_CERT, &AWS_ROOT_CA2_SHA256)
            .expect("Failed to make AWS CA2 cert");
        make_cert(AWS_ROOT_CA3_CERT, &AWS_ROOT_CA3_SHA256)
            .expect("Failed to make AWS CA3 cert");
        make_cert(AWS_ROOT_CA4_CERT, &AWS_ROOT_CA4_SHA256)
            .expect("Failed to make AWS CA4 cert");
        make_cert(SFS_ROOT_G2_CERT, &SFS_ROOT_G2_SHA256)
            .expect("Failed to make SFS G2 cert");

        make_cert(GTS_ROOT_R1_CERT, &GTS_ROOT_R1_SHA256)
            .expect("Failed to make GTS R1 cert");
        make_cert(GTS_ROOT_R2_CERT, &GTS_ROOT_R2_SHA256)
            .expect("Failed to make GTS R2 cert");
        make_cert(GTS_ROOT_R3_CERT, &GTS_ROOT_R3_SHA256)
            .expect("Failed to make GTS R3 cert");
        make_cert(GTS_ROOT_R4_CERT, &GTS_ROOT_R4_SHA256)
            .expect("Failed to make GTS R4 cert");
    }
}
