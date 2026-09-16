#![forbid(unsafe_code)]

use std::convert::Infallible;

use hpke::{
    aead::{AeadTag, AesGcm256},
    kdf::HkdfSha256,
    kem::X25519HkdfSha256,
    Deserializable, Kem, OpModeR, OpModeS, Serializable,
};
use rand_core::{TryCryptoRng, TryRng};
use zeroize::Zeroizing;

type Profile = X25519HkdfSha256;

const INFO: &[u8] = b"worldcoin/ipcp/hpke/v1";
const AAD: &[u8] = b"";
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
pub const PAYLOAD_OVERHEAD: usize = KEY_LEN + TAG_LEN;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpcpImageHpkePayload {
    pub enc: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid X25519 key")]
    InvalidKey,
    #[error("Invalid encrypted iPCP image payload")]
    InvalidPayload,
    #[error("System randomness unavailable")]
    Randomness,
    #[error("iPCP image encryption failed")]
    Encryption,
    #[error("iPCP image decryption failed")]
    Decryption,
}

pub fn encrypt_ipcp_image_payload(
    orb_public_key: &[u8],
    ipcp_image: &[u8],
) -> Result<IpcpImageHpkePayload, Error> {
    encrypt_with_entropy(orb_public_key, ipcp_image, getrandom::fill)
}

fn encrypt_with_entropy(
    orb_public_key: &[u8],
    plaintext: &[u8],
    fill: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
) -> Result<IpcpImageHpkePayload, Error> {
    let mut entropy = EphemeralEntropy {
        bytes: Zeroizing::new([0; KEY_LEN]),
        position: 0,
    };
    fill(entropy.bytes.as_mut()).map_err(|_| Error::Randomness)?;
    seal(orb_public_key, plaintext, INFO, AAD, &mut entropy)
}

fn seal(
    orb_public_key: &[u8],
    plaintext: &[u8],
    info: &[u8],
    aad: &[u8],
    entropy: &mut EphemeralEntropy,
) -> Result<IpcpImageHpkePayload, Error> {
    let public_key = <Profile as Kem>::PublicKey::from_bytes(orb_public_key)
        .map_err(|_| Error::InvalidKey)?;
    let len = plaintext
        .len()
        .checked_add(TAG_LEN)
        .ok_or(Error::InvalidPayload)?;
    let tag_start = len - TAG_LEN;
    let mut ciphertext = Zeroizing::new(vec![0; len]);
    ciphertext[..tag_start].copy_from_slice(plaintext);
    let (enc, tag) = hpke::single_shot_seal_inout_detached_with_rng::<
        AesGcm256,
        HkdfSha256,
        Profile,
    >(
        &OpModeS::Base,
        &public_key,
        info,
        ciphertext[..tag_start].as_mut().into(),
        aad,
        entropy,
    )
    .map_err(|_| Error::Encryption)?;
    ciphertext[tag_start..].copy_from_slice(&tag.to_bytes());
    Ok(IpcpImageHpkePayload {
        enc: enc.to_bytes().to_vec(),
        ciphertext: std::mem::take(&mut *ciphertext),
    })
}

pub fn decrypt_ipcp_image_payload(
    orb_private_key: &[u8],
    payload: &IpcpImageHpkePayload,
) -> Result<Zeroizing<Vec<u8>>, Error> {
    open(orb_private_key, payload, INFO, AAD)
}

fn open(
    orb_private_key: &[u8],
    payload: &IpcpImageHpkePayload,
    info: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, Error> {
    if payload.enc.len() != KEY_LEN || payload.ciphertext.len() < TAG_LEN {
        return Err(Error::InvalidPayload);
    }
    let tag_start = payload.ciphertext.len() - TAG_LEN;
    let private_key = <Profile as Kem>::PrivateKey::from_bytes(orb_private_key)
        .map_err(|_| Error::InvalidKey)?;
    let enc = <Profile as Kem>::EncappedKey::from_bytes(&payload.enc)
        .map_err(|_| Error::InvalidPayload)?;
    let tag = AeadTag::<AesGcm256>::from_bytes(&payload.ciphertext[tag_start..])
        .map_err(|_| Error::InvalidPayload)?;
    let mut plaintext = Zeroizing::new(payload.ciphertext[..tag_start].to_vec());
    hpke::single_shot_open_inout_detached::<AesGcm256, HkdfSha256, Profile>(
        &OpModeR::Base,
        &private_key,
        &enc,
        info,
        plaintext.as_mut_slice().into(),
        aad,
        &tag,
    )
    .map_err(|_| Error::Decryption)?;
    Ok(plaintext)
}

struct EphemeralEntropy {
    bytes: Zeroizing<[u8; KEY_LEN]>,
    position: usize,
}

impl TryRng for EphemeralEntropy {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut bytes = Zeroizing::new([0; 4]);
        self.try_fill_bytes(bytes.as_mut())?;
        Ok(u32::from_le_bytes(*bytes))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut bytes = Zeroizing::new([0; 8]);
        self.try_fill_bytes(bytes.as_mut())?;
        Ok(u64::from_le_bytes(*bytes))
    }

    fn try_fill_bytes(&mut self, output: &mut [u8]) -> Result<(), Infallible> {
        let end = self
            .position
            .checked_add(output.len())
            .expect("Entropy overflow");
        output.copy_from_slice(
            self.bytes
                .get(self.position..end)
                .expect("X25519 entropy exhausted"),
        );
        self.position = end;
        Ok(())
    }
}

impl TryCryptoRng for EphemeralEntropy {}

#[cfg(test)]
mod tests;
