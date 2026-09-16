#![forbid(unsafe_code)]

use hpke::{
    aead::{AeadTag, AesGcm256},
    kdf::HkdfSha256,
    kem::X25519HkdfSha256,
    Deserializable, Kem, OpModeR, OpModeS, Serializable,
};
pub use orb_relay_messages::common::v1::IpcpHpkePayload as EncryptedPayload;
use zeroize::Zeroizing;

type Profile = X25519HkdfSha256;

const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
pub const PAYLOAD_OVERHEAD: usize = KEY_LEN + TAG_LEN;

pub struct PairingKey {
    pairing_private_key: <Profile as Kem>::PrivateKey,
    pub pairing_public_key: [u8; KEY_LEN],
}

impl PairingKey {
    pub fn new_pairing_key() -> Self {
        let (pairing_private_key, pairing_public_key) = Profile::gen_keypair();
        Self {
            pairing_private_key,
            pairing_public_key: pairing_public_key.to_bytes().into(),
        }
    }

    pub fn decrypt(
        &self,
        encrypted_payload: &EncryptedPayload,
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
            &[],
            plaintext.as_mut_slice().into(),
            &[],
            &tag,
        )
        .map_err(|_| Error::Decryption)?;
        Ok(plaintext)
    }

    pub fn encrypt(
        recipient_public_key: &[u8],
        plaintext: Vec<u8>,
    ) -> Result<EncryptedPayload, Error> {
        let plaintext = Zeroizing::new(plaintext);
        let recipient_public_key =
            <Profile as Kem>::PublicKey::from_bytes(recipient_public_key)
                .map_err(|_| Error::InvalidKey)?;
        let len = plaintext
            .len()
            .checked_add(TAG_LEN)
            .ok_or(Error::InvalidPayload)?;
        let tag_start = len - TAG_LEN;
        let mut ciphertext = Zeroizing::new(vec![0; len]);
        ciphertext[..tag_start].copy_from_slice(&plaintext);
        let (ephemeral_public_key, tag) =
            hpke::single_shot_seal_inout_detached::<AesGcm256, HkdfSha256, Profile>(
                &OpModeS::Base,
                &recipient_public_key,
                &[],
                ciphertext[..tag_start].as_mut().into(),
                &[],
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
    #[error("HPKE encryption failed")]
    Encryption,
    #[error("HPKE decryption failed")]
    Decryption,
}

#[cfg(test)]
mod tests;
