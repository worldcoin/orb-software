use std::sync::OnceLock;

use eyre::{ensure, Result, WrapErr};
use hex_literal::hex;
use reqwest::{Certificate, Client, ClientBuilder};

pub use reqwest;

const AWS_ROOT_CA_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/AmazonRootCA1.pem"
));
static AWS_ROOT_CA_SHA256: [u8; 32] =
    hex!("2c43952ee9e000ff2acc4e2ed0897c0a72ad5fa72c3d934e81741cbd54f05bd1");

static GTS_ROOT_R1_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/GTS_Root_R1.pem"
));
static GTS_ROOT_R1_SHA256: [u8; 32] =
    hex!("4195ea007a7ef8d3e2d338e8d9ff0083198e36bfa025442ddf41bb5213904fc2");

static GTS_ROOT_R2_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/GTS_Root_R2.pem"
));
static GTS_ROOT_R2_SHA256: [u8; 32] =
    hex!("1a49076630e489e4b1056804fb6c768397a9de52b236609aaf6ec5b94ce508ec");

static GTS_ROOT_R3_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/GTS_Root_R3.pem"
));
static GTS_ROOT_R3_SHA256: [u8; 32] =
    hex!("39238e09bb7d30e39fbf87746ceac206f7ec206cff3d73c743e3f818ca2ec54f");

static GTS_ROOT_R4_CERT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/certs/GTS_Root_R4.pem"
));
static GTS_ROOT_R4_SHA256: [u8; 32] =
    hex!("7e8b80d078d3dd77d3ed2108dd2b33412c12d7d72cb0965741c70708691776a2");

/// Important certificates we vendor for security
#[derive(Debug)]
pub struct VendoredCerts {
    /// AWS Root CA
    pub aws_root_ca: Certificate,
    /// Google Trust Services Root CAs
    pub gts_root_r1: Certificate,
    pub gts_root_r2: Certificate,
    pub gts_root_r3: Certificate,
    pub gts_root_r4: Certificate,

}

pub fn get_certs() -> &'static VendoredCerts {
    static CERTS: OnceLock<VendoredCerts> = OnceLock::new();
    CERTS.get_or_init(|| {
        let aws_root_ca = make_cert(AWS_ROOT_CA_CERT, &AWS_ROOT_CA_SHA256)
            .expect("Failed to make AWS cert");
        let gts_root_r1 = make_cert(GTS_ROOT_R1_CERT, &GTS_ROOT_R1_SHA256)
            .expect("Failed to make GTS R1 cert");
        let gts_root_r2 = make_cert(GTS_ROOT_R2_CERT, &GTS_ROOT_R2_SHA256)
            .expect("Failed to make GTS R2 cert");
        let gts_root_r3 = make_cert(GTS_ROOT_R3_CERT, &GTS_ROOT_R3_SHA256)
            .expect("Failed to make GTS R3 cert");
        let gts_root_r4 = make_cert(GTS_ROOT_R4_CERT, &GTS_ROOT_R4_SHA256)
            .expect("Failed to make GTS R4 cert");

        VendoredCerts {
            aws_root_ca,
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
        .min_tls_version(reqwest::tls::Version::TLS_1_3)
        .tls_built_in_root_certs(false)
        .https_only(true)
        .add_root_certificate(certs.aws_root_ca.clone())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_certs() {
        make_cert(AWS_ROOT_CA_CERT, &AWS_ROOT_CA_SHA256)
            .expect("Failed to make AWS cert");
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
