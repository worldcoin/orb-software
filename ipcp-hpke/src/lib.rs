#![forbid(unsafe_code)]

use std::{convert::Infallible, future::Future};

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

pub struct PairingKey {
    orb_private_key: <Profile as Kem>::PrivateKey,
    orb_public_key: [u8; KEY_LEN],
}

impl PairingKey {
    pub fn new() -> Result<Self, Error> {
        Self::with_randomness(getrandom::fill)
    }

    fn with_randomness(
        fill: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
    ) -> Result<Self, Error> {
        let mut orb_private_key_bytes = Zeroizing::new([0; KEY_LEN]);
        fill(orb_private_key_bytes.as_mut()).map_err(|_| Error::Randomness)?;
        let orb_private_key =
            <Profile as Kem>::PrivateKey::from_bytes(orb_private_key_bytes.as_ref())
                .map_err(|_| Error::InvalidKey)?;
        let orb_public_key = Profile::sk_to_pk(&orb_private_key).to_bytes().into();
        Ok(Self {
            orb_private_key,
            orb_public_key,
        })
    }

    pub fn public_key(&self) -> &[u8; KEY_LEN] {
        &self.orb_public_key
    }

    pub fn decrypt_ipcp_image_payload(
        &self,
        encrypted_ipcp_image_payload: &IpcpImageHpkePayload,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        decrypt_ipcp_image_with_context(
            &self.orb_private_key,
            encrypted_ipcp_image_payload,
            INFO,
            AAD,
        )
    }
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
    #[error("iPCP image encryption task failed")]
    EncryptionTask(#[source] tokio::task::JoinError),
    #[error("iPCP image decryption failed")]
    Decryption,
}

pub fn encrypt_ipcp_image_payload(
    orb_public_key: Vec<u8>,
    ipcp_image: Vec<u8>,
) -> impl Future<Output = Result<IpcpImageHpkePayload, Error>> + Send {
    let ipcp_image = Zeroizing::new(ipcp_image);
    async move {
        tokio::task::spawn_blocking(move || {
            encrypt_ipcp_image_with_randomness(
                &orb_public_key,
                &ipcp_image,
                getrandom::fill,
            )
        })
        .await
        .map_err(Error::EncryptionTask)?
    }
}

fn encrypt_ipcp_image_with_randomness(
    orb_public_key: &[u8],
    ipcp_image: &[u8],
    fill: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
) -> Result<IpcpImageHpkePayload, Error> {
    let mut app_ephemeral_key_material = AppEphemeralKeyMaterial {
        bytes: Zeroizing::new([0; KEY_LEN]),
        position: 0,
    };
    fill(app_ephemeral_key_material.bytes.as_mut()).map_err(|_| Error::Randomness)?;
    encrypt_ipcp_image_with_context(
        orb_public_key,
        ipcp_image,
        INFO,
        AAD,
        &mut app_ephemeral_key_material,
    )
}

fn encrypt_ipcp_image_with_context(
    orb_public_key_bytes: &[u8],
    ipcp_image: &[u8],
    info: &[u8],
    aad: &[u8],
    app_ephemeral_key_material: &mut AppEphemeralKeyMaterial,
) -> Result<IpcpImageHpkePayload, Error> {
    let orb_public_key = <Profile as Kem>::PublicKey::from_bytes(orb_public_key_bytes)
        .map_err(|_| Error::InvalidKey)?;
    let len = ipcp_image
        .len()
        .checked_add(TAG_LEN)
        .ok_or(Error::InvalidPayload)?;
    let tag_start = len - TAG_LEN;
    let mut ciphertext = Zeroizing::new(vec![0; len]);
    ciphertext[..tag_start].copy_from_slice(ipcp_image);
    let (app_public_key, tag) = hpke::single_shot_seal_inout_detached_with_rng::<
        AesGcm256,
        HkdfSha256,
        Profile,
    >(
        &OpModeS::Base,
        &orb_public_key,
        info,
        ciphertext[..tag_start].as_mut().into(),
        aad,
        app_ephemeral_key_material,
    )
    .map_err(|_| Error::Encryption)?;
    ciphertext[tag_start..].copy_from_slice(&tag.to_bytes());
    Ok(IpcpImageHpkePayload {
        enc: app_public_key.to_bytes().to_vec(),
        ciphertext: std::mem::take(&mut *ciphertext),
    })
}

fn decrypt_ipcp_image_with_context(
    orb_private_key: &<Profile as Kem>::PrivateKey,
    encrypted_ipcp_image_payload: &IpcpImageHpkePayload,
    info: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, Error> {
    if encrypted_ipcp_image_payload.enc.len() != KEY_LEN
        || encrypted_ipcp_image_payload.ciphertext.len() < TAG_LEN
    {
        return Err(Error::InvalidPayload);
    }
    let tag_start = encrypted_ipcp_image_payload.ciphertext.len() - TAG_LEN;
    let app_public_key =
        <Profile as Kem>::EncappedKey::from_bytes(&encrypted_ipcp_image_payload.enc)
            .map_err(|_| Error::InvalidPayload)?;
    let tag = AeadTag::<AesGcm256>::from_bytes(
        &encrypted_ipcp_image_payload.ciphertext[tag_start..],
    )
    .map_err(|_| Error::InvalidPayload)?;
    let mut decrypted_ipcp_image_bytes =
        Zeroizing::new(encrypted_ipcp_image_payload.ciphertext[..tag_start].to_vec());
    hpke::single_shot_open_inout_detached::<AesGcm256, HkdfSha256, Profile>(
        &OpModeR::Base,
        orb_private_key,
        &app_public_key,
        info,
        decrypted_ipcp_image_bytes.as_mut_slice().into(),
        aad,
        &tag,
    )
    .map_err(|_| Error::Decryption)?;
    Ok(decrypted_ipcp_image_bytes)
}

struct AppEphemeralKeyMaterial {
    bytes: Zeroizing<[u8; KEY_LEN]>,
    position: usize,
}

impl TryRng for AppEphemeralKeyMaterial {
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
            .expect("App key material offset overflow");
        output.copy_from_slice(
            self.bytes
                .get(self.position..end)
                .expect("App key material exhausted"),
        );
        self.position = end;
        Ok(())
    }
}

impl TryCryptoRng for AppEphemeralKeyMaterial {}

#[cfg(test)]
mod tests;
