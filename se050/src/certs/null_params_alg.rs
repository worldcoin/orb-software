use rustls_pki_types::{
    AlgorithmIdentifier, InvalidSignature, SignatureVerificationAlgorithm,
};

/// Wrapper that delegates to a built-in webpki algorithm but overrides the
/// `signature_alg_id` to match NXP's non-conformant encoding that includes
/// explicit NULL parameters (e.g. `05 00`) after the OID.
///
/// Per RFC 5758, ECDSA AlgorithmIdentifiers MUST omit the parameters field,
/// but NXP's SE050 cert chain includes them.
#[derive(Debug)]
struct NullParamsAlg {
    inner: &'static dyn SignatureVerificationAlgorithm,
    sig_alg_id: AlgorithmIdentifier,
}

impl SignatureVerificationAlgorithm for NullParamsAlg {
    fn public_key_alg_id(&self) -> AlgorithmIdentifier {
        self.inner.public_key_alg_id()
    }

    fn signature_alg_id(&self) -> AlgorithmIdentifier {
        self.sig_alg_id
    }

    fn verify_signature(
        &self,
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), InvalidSignature> {
        self.inner.verify_signature(public_key, message, signature)
    }
}

// ecdsa-with-SHA256 OID + explicit NULL: 06 08 2a 86 48 ce 3d 04 03 02 05 00
const ECDSA_SHA256_NULL: AlgorithmIdentifier = AlgorithmIdentifier::from_slice(&[
    0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02, 0x05, 0x00,
]);

/// ECDSA P-256/SHA-256 accepting the NULL-params encoding in the cert.
static ECDSA_P256_SHA256_NULL: &dyn SignatureVerificationAlgorithm = &NullParamsAlg {
    inner: webpki::aws_lc_rs::ECDSA_P256_SHA256,
    sig_alg_id: ECDSA_SHA256_NULL,
};

/// ECDSA P-521/SHA-256 accepting the NULL-params encoding in the cert.
/// The NXP intermediate cert is signed by the P-521 root using SHA-256.
static ECDSA_P521_SHA256_NULL: &dyn SignatureVerificationAlgorithm = &NullParamsAlg {
    inner: webpki::aws_lc_rs::ECDSA_P521_SHA256,
    sig_alg_id: ECDSA_SHA256_NULL,
};

/// P256 and P521 SHA256 standard variants plus variants that accept NULL
/// parameters in the AlgorithmIdentifier. Necessary because these NULL params
/// appear in the NXP SE050 certs, despite being non-conformant.
pub static NXP_VERIFICATION_ALGS: &[&dyn SignatureVerificationAlgorithm] = &[
    // NULL-params variants for NXP certs
    ECDSA_P256_SHA256_NULL,
    ECDSA_P521_SHA256_NULL,
    // // Standard algorithms (inlined since we can't concat const slices)
    webpki::aws_lc_rs::ECDSA_P256_SHA256,
    webpki::aws_lc_rs::ECDSA_P521_SHA256,
];
