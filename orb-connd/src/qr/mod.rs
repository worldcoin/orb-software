use base64::Engine as _;
use color_eyre::{
    eyre::{eyre, Context, ContextCompat},
    Result,
};
use orb_info::orb_os_release::OrbRelease;
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_ASN1};

static PROD: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE1Rwr5CEvtWzmcQu4IS+VFmkRiZdM
SmNKUZ+THL5nRV2kYmNRc6fBBFiam5HjYRlbFGKjctJZ3gXQz4Bv30+FOw==
-----END PUBLIC KEY-----";

static STAGE: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEAfVD06rPhda6auRt3cK+Ntqrz5Fo
E5StFkWbhShXco5lwJPtZitWdElNxaCzMmJiyF6AXyd11SRzxE4FjUZp8Q==
-----END PUBLIC KEY-----";

fn pem_to_der_spki(pem: &str) -> Result<Vec<u8>> {
    let b64: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .map(str::trim)
        .collect();

    println!("b64: {b64}");

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .wrap_err("failed to decode b64 pem")?;

    Ok(bytes)
}

// verifies the sig or netconfig qr with ECC_NIST_P256 pub key
pub fn verify(qr: &str, release: OrbRelease) -> Result<()> {
    let pub_key = match release {
        OrbRelease::Prod | OrbRelease::Analysis => PROD,
        _ => STAGE,
    };

    // Use exact bytes up to ";SIG:"
    let sig_pos = qr.find("SIG:").wrap_err("SIG not found")?;
    let msg_bytes = &qr.as_bytes()[..sig_pos];
    println!("msg: {}", qr[..sig_pos].to_string());

    // Decode ASN.1/DER signature; only trim newline terminators
    let sig_b64 = &qr[sig_pos + 4..];
    println!("SIG: {sig_b64}");
    let sig_der = base64::engine::general_purpose::STANDARD
        .decode(sig_b64.trim_end_matches(['\r', '\n']))
        .wrap_err("bad base64 sig")?;

    // SPKI DER for the verifying key
    let spki_der = pem_to_der_spki(pub_key)?;
    let vk = UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, &spki_der);

    vk.verify(msg_bytes, &sig_der)
        .map_err(|e| eyre!("verification of qr sig failed: {e}"))
}
