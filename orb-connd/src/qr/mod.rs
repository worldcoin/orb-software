use base64::Engine as _;
use color_eyre::{
    eyre::{eyre, Context, ContextCompat},
    Result,
};
use orb_info::orb_os_release::OrbRelease;
use p256::{
    ecdsa::{signature::Verifier, Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
};

static PROD: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE1Rwr5CEvtWzmcQu4IS+VFmkRiZdM
SmNKUZ+THL5nRV2kYmNRc6fBBFiam5HjYRlbFGKjctJZ3gXQz4Bv30+FOw==
-----END PUBLIC KEY-----";

static STAGE: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEAfVD06rPhda6auRt3cK+Ntqrz5Fo
E5StFkWbhShXco5lwJPtZitWdElNxaCzMmJiyF6AXyd11SRzxE4FjUZp8Q==
-----END PUBLIC KEY-----";

// verifies the sig or netconfig qr with ECC_NIST_P256 pub key
pub fn verify(qr: &str, release: OrbRelease) -> Result<()> {
    let pub_key = match release {
        OrbRelease::Dev => STAGE,
        _ => PROD,
    };

    let (msg, sig) = qr.split_once("SIG:").wrap_err("SIG not found")?;

    let sig_der = base64::engine::general_purpose::STANDARD
        .decode(sig.trim_end_matches(['\r', '\n']))
        .wrap_err("bad base64 sig")?;

    let sig = Signature::from_der(&sig_der)?;
    let sig = match sig.normalize_s() {
        None => sig,
        Some(normalized) => normalized,
    };

    VerifyingKey::from_public_key_pem(pub_key)?
        .verify(msg.as_bytes(), &sig)
        .map_err(|e| eyre!("verification of qr sig failed: {e}"))
}
