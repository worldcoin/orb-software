use eyre::{ensure, Result};
use hex_literal::hex;

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

pub fn all_pem_certs() -> [&'static [u8]; 9] {
    [
        AWS_ROOT_CA1_CERT,
        AWS_ROOT_CA2_CERT,
        AWS_ROOT_CA3_CERT,
        AWS_ROOT_CA4_CERT,
        SFS_ROOT_G2_CERT,
        GTS_ROOT_R1_CERT,
        GTS_ROOT_R2_CERT,
        GTS_ROOT_R3_CERT,
        GTS_ROOT_R4_CERT,
    ]
}

pub fn verify_pinned_cert(cert_pem: &[u8], sha256: &[u8; 32]) -> Result<()> {
    // Verify that the certificate has not been replaced.
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    context.update(cert_pem);
    let digest = context.finish();
    ensure!(
        digest.as_ref() == sha256,
        "provided cert bytes did not match sha256 hash"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn test_verify_pinned_cert() {
        verify_pinned_cert(AWS_ROOT_CA1_CERT, &AWS_ROOT_CA1_SHA256)
            .expect("Failed to verify AWS CA1 cert");
        verify_pinned_cert(AWS_ROOT_CA2_CERT, &AWS_ROOT_CA2_SHA256)
            .expect("Failed to verify AWS CA2 cert");
        verify_pinned_cert(AWS_ROOT_CA3_CERT, &AWS_ROOT_CA3_SHA256)
            .expect("Failed to verify AWS CA3 cert");
        verify_pinned_cert(AWS_ROOT_CA4_CERT, &AWS_ROOT_CA4_SHA256)
            .expect("Failed to verify AWS CA4 cert");
        verify_pinned_cert(SFS_ROOT_G2_CERT, &SFS_ROOT_G2_SHA256)
            .expect("Failed to verify SFS G2 cert");

        verify_pinned_cert(GTS_ROOT_R1_CERT, &GTS_ROOT_R1_SHA256)
            .expect("Failed to verify GTS R1 cert");
        verify_pinned_cert(GTS_ROOT_R2_CERT, &GTS_ROOT_R2_SHA256)
            .expect("Failed to verify GTS R2 cert");
        verify_pinned_cert(GTS_ROOT_R3_CERT, &GTS_ROOT_R3_SHA256)
            .expect("Failed to verify GTS R3 cert");
        verify_pinned_cert(GTS_ROOT_R4_CERT, &GTS_ROOT_R4_SHA256)
            .expect("Failed to verify GTS R4 cert");
    }

    const EXPIRATION_GATE: Duration = Duration::from_secs(180 * 24 * 60 * 60);

    #[test]
    fn pinned_certificates_do_not_expire_soon() {
        for (name, cert_pem) in [
            ("AmazonRootCA1.pem", AWS_ROOT_CA1_CERT),
            ("AmazonRootCA2.pem", AWS_ROOT_CA2_CERT),
            ("AmazonRootCA3.pem", AWS_ROOT_CA3_CERT),
            ("AmazonRootCA4.pem", AWS_ROOT_CA4_CERT),
            ("SFSRootCAG2.pem", SFS_ROOT_G2_CERT),
            ("GTS_Root_R1.pem", GTS_ROOT_R1_CERT),
            ("GTS_Root_R2.pem", GTS_ROOT_R2_CERT),
            ("GTS_Root_R3.pem", GTS_ROOT_R3_CERT),
            ("GTS_Root_R4.pem", GTS_ROOT_R4_CERT),
        ] {
            let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem)
                .unwrap_or_else(|e| panic!("{name} should parse as PEM: {e}"));
            let cert = pem
                .parse_x509()
                .unwrap_or_else(|e| panic!("{name} should parse as X.509: {e}"));

            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("system time should be after UNIX epoch");
            let expiry = Duration::from_secs(
                cert.validity()
                    .not_after
                    .timestamp()
                    .try_into()
                    .expect("certificate expiry should be after UNIX epoch"),
            );
            let remaining = expiry.saturating_sub(now);

            assert!(
                remaining >= EXPIRATION_GATE,
                "{name} expires in {remaining:?}"
            );
        }
    }
}
