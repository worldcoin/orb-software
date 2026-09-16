#![forbid(unsafe_code)]

use std::convert::Infallible;

use hpke::{
    aead::{AeadTag, AesGcm256},
    kdf::HkdfSha256,
    kem::X25519HkdfSha256,
    Deserializable, Kem, OpModeR, OpModeS, Serializable,
};
pub use orb_relay_messages::common::v1::IpcpHpkePayload as EncryptedPayload;
use rand_core::{TryCryptoRng, TryRng};
use zeroize::Zeroizing;

type Profile = X25519HkdfSha256;

const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
pub const PAYLOAD_OVERHEAD: usize = KEY_LEN + TAG_LEN;

pub struct PairingKey {
    pairing_private_key: <Profile as Kem>::PrivateKey,
    pairing_public_key: [u8; KEY_LEN],
}

impl PairingKey {
    pub fn new() -> Result<Self, Error> {
        Self::with_randomness(getrandom::fill)
    }

    fn with_randomness(
        fill: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
    ) -> Result<Self, Error> {
        let mut pairing_private_key_bytes = Zeroizing::new([0; KEY_LEN]);
        fill(pairing_private_key_bytes.as_mut()).map_err(|_| Error::Randomness)?;
        let pairing_private_key = <Profile as Kem>::PrivateKey::from_bytes(
            pairing_private_key_bytes.as_ref(),
        )
        .map_err(|_| Error::InvalidKey)?;
        let pairing_public_key =
            Profile::sk_to_pk(&pairing_private_key).to_bytes().into();
        Ok(Self {
            pairing_private_key,
            pairing_public_key,
        })
    }

    pub fn pairing_public_key(&self) -> &[u8; KEY_LEN] {
        &self.pairing_public_key
    }

    pub fn decrypt(
        &self,
        encrypted_payload: &EncryptedPayload,
        info: &[u8],
        aad: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        if encrypted_payload.enc.len() != KEY_LEN
            || encrypted_payload.ciphertext.len() < TAG_LEN
        {
            return Err(Error::InvalidPayload);
        }
        let tag_start = encrypted_payload.ciphertext.len() - TAG_LEN;
        let ephemeral_public_key =
            <Profile as Kem>::EncappedKey::from_bytes(&encrypted_payload.enc)
                .map_err(|_| Error::InvalidPayload)?;
        let tag = AeadTag::<AesGcm256>::from_bytes(
            &encrypted_payload.ciphertext[tag_start..],
        )
        .map_err(|_| Error::InvalidPayload)?;
        let mut plaintext =
            Zeroizing::new(encrypted_payload.ciphertext[..tag_start].to_vec());
        hpke::single_shot_open_inout_detached::<AesGcm256, HkdfSha256, Profile>(
            &OpModeR::Base,
            &self.pairing_private_key,
            &ephemeral_public_key,
            info,
            plaintext.as_mut_slice().into(),
            aad,
            &tag,
        )
        .map_err(|_| Error::Decryption)?;
        Ok(plaintext)
    }

    pub fn encrypt(
        recipient_public_key: &[u8],
        plaintext: Vec<u8>,
        info: &[u8],
        aad: &[u8],
    ) -> Result<EncryptedPayload, Error> {
        let plaintext = Zeroizing::new(plaintext);
        Self::encrypt_with_randomness(
            recipient_public_key,
            &plaintext,
            info,
            aad,
            getrandom::fill,
        )
    }

    fn encrypt_with_randomness(
        recipient_public_key: &[u8],
        plaintext: &[u8],
        info: &[u8],
        aad: &[u8],
        fill: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
    ) -> Result<EncryptedPayload, Error> {
        let recipient_public_key =
            <Profile as Kem>::PublicKey::from_bytes(recipient_public_key)
                .map_err(|_| Error::InvalidKey)?;
        let mut ephemeral_key_material = EphemeralKeyMaterial {
            bytes: Zeroizing::new([0; KEY_LEN]),
            position: 0,
        };
        fill(ephemeral_key_material.bytes.as_mut()).map_err(|_| Error::Randomness)?;
        Self::encrypt_with_context(
            &recipient_public_key,
            plaintext,
            info,
            aad,
            &mut ephemeral_key_material,
        )
    }

    fn encrypt_with_context(
        recipient_public_key: &<Profile as Kem>::PublicKey,
        plaintext: &[u8],
        info: &[u8],
        aad: &[u8],
        ephemeral_key_material: &mut EphemeralKeyMaterial,
    ) -> Result<EncryptedPayload, Error> {
        let len = plaintext
            .len()
            .checked_add(TAG_LEN)
            .ok_or(Error::InvalidPayload)?;
        let tag_start = len - TAG_LEN;
        let mut ciphertext = Zeroizing::new(vec![0; len]);
        ciphertext[..tag_start].copy_from_slice(plaintext);
        let (ephemeral_public_key, tag) =
            hpke::single_shot_seal_inout_detached_with_rng::<
                AesGcm256,
                HkdfSha256,
                Profile,
            >(
                &OpModeS::Base,
                recipient_public_key,
                info,
                ciphertext[..tag_start].as_mut().into(),
                aad,
                ephemeral_key_material,
            )
            .map_err(|_| Error::Encryption)?;
        ciphertext[tag_start..].copy_from_slice(&tag.to_bytes());
        Ok(EncryptedPayload {
            enc: ephemeral_public_key.to_bytes().to_vec(),
            ciphertext: std::mem::take(&mut *ciphertext),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid X25519 key")]
    InvalidKey,
    #[error("Invalid encrypted payload")]
    InvalidPayload,
    #[error("System randomness unavailable")]
    Randomness,
    #[error("HPKE encryption failed")]
    Encryption,
    #[error("HPKE decryption failed")]
    Decryption,
}

struct EphemeralKeyMaterial {
    bytes: Zeroizing<[u8; KEY_LEN]>,
    position: usize,
}

impl TryRng for EphemeralKeyMaterial {
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
            .expect("Ephemeral key material offset overflow");
        output.copy_from_slice(
            self.bytes
                .get(self.position..end)
                .expect("Ephemeral key material exhausted"),
        );
        self.position = end;
        Ok(())
    }
}

impl TryCryptoRng for EphemeralKeyMaterial {}

#[cfg(test)]
mod tests;
