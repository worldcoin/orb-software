use bon::Builder;

use crate::{
    certs::ChipUniquePubkey,
    extra_data::{ChipId, Freshness, Timestamp},
};

/// See AN12413 section 4.7.3.1
#[derive(Builder, Debug, Clone, Eq, PartialEq)]
pub struct Attestation {
    pubkey: Vec<u8>, // TODO: newtype this
    attrs: (),
    timestamp: Timestamp,
    freshness: Freshness,
    chip_id: ChipId,
    sig: AttestationSig,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValidatedAttestation(Attestation);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AttestationSig(p256::ecdsa::Signature);

#[derive(Debug, thiserror::Error)]
pub enum AttributesErr {}

#[derive(Debug, thiserror::Error)]
pub enum SignatureErr {}

#[derive(Debug, thiserror::Error)]
pub enum ValidateErr {
    #[error("invalid attributes: {0}")]
    Attributes(#[from] AttributesErr),
    #[error("invalid signature: {0}")]
    Signature(#[from] SignatureErr),
}

impl Attestation {
    fn validate_sig(&self, _pubkey: &ChipUniquePubkey) -> Result<(), SignatureErr> {
        todo!()
    }

    fn validate_attrs(&self) -> Result<(), AttributesErr> {
        todo!()
    }

    pub fn validate(
        &self,
        chip_unique_pubkey: &ChipUniquePubkey,
    ) -> Result<(), ValidateErr> {
        self.validate_attrs()?;
        self.validate_sig(chip_unique_pubkey)?;

        Ok(())
    }
}
