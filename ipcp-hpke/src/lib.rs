#![forbid(unsafe_code)]

use hpke::{
    aead::AesGcm256, kdf::HkdfSha256, kem::X25519HkdfSha256, Deserializable, Kem,
    OpModeR, OpModeS, Serializable,
};
pub use orb_relay_messages::common::v1::IpcpHpkePayload as EncryptedPayload;
use zeroize::Zeroizing;

type Profile = X25519HkdfSha256;

const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
pub const PAYLOAD_OVERHEAD: usize = KEY_LEN + TAG_LEN;

pub struct PairingKey {
    sk: <Profile as Kem>::PrivateKey,
    pub pk: <Profile as Kem>::PublicKey,
}

impl Default for PairingKey {
    fn default() -> Self {
        Self::new()
    }
}

impl PairingKey {
    pub fn new() -> Self {
        let (sk, pk) = Profile::gen_keypair();
        Self { sk, pk }
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
        let ephemeral_public_key =
            <Profile as Kem>::EncappedKey::from_bytes(&encrypted_payload.enc)
                .map_err(|_| Error::InvalidPayload)?;
        hpke::single_shot_open::<AesGcm256, HkdfSha256, Profile>(
            &OpModeR::Base,
            &self.sk,
            &ephemeral_public_key,
            &[],
            &encrypted_payload.ciphertext,
            &[],
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::Decryption)
    }

    pub fn encrypt(
        recipient_pk: &[u8],
        plaintext: Zeroizing<Vec<u8>>,
    ) -> Result<EncryptedPayload, Error> {
        let recipient_pk = <Profile as Kem>::PublicKey::from_bytes(recipient_pk)
            .map_err(|_| Error::InvalidKey)?;
        let (ephemeral_public_key, ciphertext) =
            hpke::single_shot_seal::<AesGcm256, HkdfSha256, Profile>(
                &OpModeS::Base,
                &recipient_pk,
                &[],
                &plaintext,
                &[],
            )
            .map_err(|_| Error::Encryption)?;
        Ok(EncryptedPayload {
            enc: ephemeral_public_key.to_bytes().to_vec(),
            ciphertext,
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
