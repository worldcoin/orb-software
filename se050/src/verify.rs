use bon::Builder;
use der::Decode;
use zerocopy::IntoBytes;

use p256::ecdsa::{signature::Verifier as _, Signature as P256Signature};

use crate::{
    attributes::ObjectAttributes,
    certs::{ChipUniquePubkey, VerifyCertErr},
    extra_data::{ChipId, ExtraData, Freshness, ParseExtraDataErr, Timestamp},
};

/// See AN12413 section 4.7.3.1
#[derive(Builder, Debug, Clone, Eq, PartialEq)]
#[builder(derive(Debug))]
pub struct Attestation<'a> {
    #[builder(setters(vis = ""))]
    pubkey_sec1_bytes: Vec<u8>, // TODO: newtype this
    attrs: &'a ObjectAttributes,
    timestamp: Timestamp,
    freshness: Freshness,
    chip_id: ChipId,
    sig: AttestationSig,
}

use self::attestation_builder as b;

#[derive(Debug, thiserror::Error)]
#[error("failed to parse extradata")]
pub struct ExtraDataBytesErr<B> {
    builder: B,
    #[source]
    err: ParseExtraDataErr,
}

impl<B> ExtraDataBytesErr<B> {
    pub fn into_builder(self) -> B {
        self.builder
    }

    pub fn into_error(self) -> ParseExtraDataErr {
        self.err
    }
}

#[derive(Debug, thiserror::Error)]
#[error("failed to parse p256 signature from DER encoded bytes")]
pub struct SignatureBytesErr<B> {
    builder: B,
    #[source]
    err: SignatureErr,
}

impl<B> SignatureBytesErr<B> {
    pub fn into_builder(self) -> B {
        self.builder
    }

    pub fn into_error(self) -> SignatureErr {
        self.err
    }
}

#[derive(Debug, thiserror::Error)]
#[error("failed to parse pubkey from DER encoded bytes")]
pub enum PubkeyBytesInnerErr {
    Der(#[from] der::Error),
    EmptyBitString,
}

#[derive(Debug, thiserror::Error)]
#[error("failed to parse pubkey from DER encoded bytes")]
pub struct PubkeyBytesErr<B> {
    builder: B,
    #[source]
    err: PubkeyBytesInnerErr,
}

impl<B> PubkeyBytesErr<B> {
    pub fn into_builder(self) -> B {
        self.builder
    }

    pub fn into_error(self) -> PubkeyBytesInnerErr {
        self.err
    }
}

type SetExtraData<S> = b::SetAttrs<b::SetTimestamp<b::SetFreshness<b::SetChipId<S>>>>;

impl<'a, S: b::State> AttestationBuilder<'a, S> {
    pub fn extra_data(
        self,
        extra_data: ExtraData<'a>,
    ) -> AttestationBuilder<'a, SetExtraData<S>>
    where
        S::Attrs: b::IsUnset,
        S::Timestamp: b::IsUnset,
        S::Freshness: b::IsUnset,
        S::ChipId: b::IsUnset,
    {
        self.chip_id(*extra_data.chip_id)
            .freshness(*extra_data.freshness)
            .timestamp(*extra_data.timestamp)
            .attrs(extra_data.object_attributes)
    }

    #[expect(clippy::result_large_err)]
    pub fn extra_data_bytes(
        self,
        extra_data: &'a [u8],
    ) -> Result<AttestationBuilder<'a, SetExtraData<S>>, ExtraDataBytesErr<Self>>
    where
        S::Attrs: b::IsUnset,
        S::Timestamp: b::IsUnset,
        S::Freshness: b::IsUnset,
        S::ChipId: b::IsUnset,
    {
        let extra_data = match ExtraData::try_from(extra_data) {
            Ok(extra_data) => extra_data,
            Err(err) => return Err(ExtraDataBytesErr { builder: self, err }),
        };

        Ok(self.extra_data(extra_data))
    }

    #[expect(clippy::result_large_err)]
    pub fn pubkey_der_bytes(
        self,
        pubkey_der: &[u8],
    ) -> Result<AttestationBuilder<'a, b::SetPubkeySec1Bytes<S>>, PubkeyBytesErr<Self>>
    where
        S::PubkeySec1Bytes: b::IsUnset,
    {
        let sec1_encoded_pubkey = match der::asn1::BitStringRef::from_der(pubkey_der) {
            Ok(bitstring) => bitstring.as_bytes().unwrap_or_default(),
            Err(err) => {
                return Err(PubkeyBytesErr {
                    builder: self,
                    err: err.into(),
                })
            }
        };

        Ok(self.pubkey_sec1_bytes(sec1_encoded_pubkey.to_vec()))
    }

    #[expect(clippy::result_large_err)]
    pub fn signature_from_der(
        self,
        signature_der: &[u8],
    ) -> Result<AttestationBuilder<'a, b::SetSig<S>>, SignatureBytesErr<Self>>
    where
        S::Sig: b::IsUnset,
    {
        let sig = match P256Signature::from_der(signature_der) {
            Ok(sig) => sig,
            Err(err) => {
                return Err(SignatureBytesErr {
                    builder: self,
                    err: err.into(),
                })
            }
        };

        Ok(self.sig(AttestationSig(sig)))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValidatedAttestation<'a>(Attestation<'a>);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AttestationSig(P256Signature);

#[derive(Debug, thiserror::Error)]
pub enum AttributesErr {}

#[derive(Debug, thiserror::Error)]
pub enum SignatureErr {
    #[error("signature invalid")]
    InvalidSig(#[from] p256::ecdsa::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ValidateErr {
    #[error("attestation had invalid attributes")]
    Attributes(#[from] AttributesErr),
    #[error("attestation had invalid signature")]
    Signature(#[from] SignatureErr),
    #[error("secure object pubkey bytes were not SEC1 encoded")]
    SecureObjectNotSec1(#[from] p256::ecdsa::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ValidateFromCertErr {
    #[error(transparent)]
    Attest(#[from] ValidateErr),
    #[error("failed to validate certificate")]
    Cert(#[from] VerifyCertErr),
}

impl<'a> Attestation<'a> {
    fn validate_sig(&self, pubkey: &ChipUniquePubkey) -> Result<(), SignatureErr> {
        let mut protected = Vec::new();
        protected.extend_from_slice(&self.pubkey_sec1_bytes);
        protected.extend(self.attrs.iter_bytes());
        protected.extend_from_slice(self.timestamp.as_bytes());
        protected.extend_from_slice(self.freshness.as_bytes());
        protected.extend_from_slice(self.chip_id.as_bytes());

        pubkey.0.verify(&protected, &self.sig.0)?;

        Ok(())
    }

    fn validate_attrs(&self) -> Result<(), AttributesErr> {
        // TODO: Make this not stubbed
        eprintln!("TODO: This is stubbed code and is a no-op");

        Ok(())
    }

    pub fn validate(
        self,
        chip_unique_pubkey: &ChipUniquePubkey,
    ) -> Result<ValidatedAttestation<'a>, ValidateErr> {
        self.validate_sig(chip_unique_pubkey)?;
        self.validate_attrs()?;

        Ok(ValidatedAttestation(self))
    }

    pub fn validate_from_cert(
        self,
        chip_cert_pem: &str,
        current_time: rustls_pki_types::UnixTime,
    ) -> Result<ValidatedAttestation<'a>, ValidateFromCertErr> {
        let chip_unique_pubkey =
            crate::certs::verify_cert(chip_cert_pem, current_time)?;

        Ok(self.validate(&chip_unique_pubkey)?)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        certs::test::TOTAL_VALID_RANGE,
        example_data::{
            ORB_ATTESTATION_KEY, ORB_ATTESTATION_KEY_EXTRA_DATA,
            ORB_ATTESTATION_KEY_SIG, ORB_IRIS_KEY, ORB_IRIS_KEY_EXTRA_DATA,
            ORB_IRIS_KEY_SIG, ORB_SESSION_KEY, ORB_SESSION_KEY_EXTRA_DATA,
            ORB_SESSION_KEY_SIG,
        },
    };

    use color_eyre::eyre::Context as _;
    use color_eyre::Result;
    use rustls_pki_types::UnixTime;

    struct Example {
        pubkey: &'static [u8],
        pubkey_is_der: bool,
        extra_data: &'static [u8],
        sig: &'static [u8],
    }

    fn check_attestation_validates(example: &Example) -> Result<()> {
        let valid_cert_time = UnixTime::since_unix_epoch(*TOTAL_VALID_RANGE.start());
        let attestation = Attestation::builder()
            .extra_data_bytes(example.extra_data)
            .map_err(|err| err.into_error())
            .wrap_err("extra data should work")?
            .signature_from_der(example.sig)
            .map_err(|err| err.into_error())
            .wrap_err("sig should work")?;
        let attestation = if example.pubkey_is_der {
            attestation
                .pubkey_der_bytes(example.pubkey)
                .wrap_err("failed to set pubkey der bytes")?
        } else {
            attestation.pubkey_sec1_bytes(example.pubkey.to_vec())
        };
        attestation
            .build()
            .validate_from_cert(crate::example_data::CERT, valid_cert_time)
            .wrap_err("attestion should validate")?;

        Ok(())
    }

    #[test]
    fn test_session_key_attestation_validates() -> Result<()> {
        let _ = color_eyre::install();
        check_attestation_validates(&Example {
            pubkey: ORB_SESSION_KEY,
            pubkey_is_der: true,
            extra_data: ORB_SESSION_KEY_EXTRA_DATA,
            sig: ORB_SESSION_KEY_SIG,
        })
    }

    #[test]
    fn test_attestation_key_attestation_validates() -> Result<()> {
        let _ = color_eyre::install();
        check_attestation_validates(&Example {
            pubkey: ORB_ATTESTATION_KEY,
            pubkey_is_der: false,
            extra_data: ORB_ATTESTATION_KEY_EXTRA_DATA,
            sig: ORB_ATTESTATION_KEY_SIG,
        })
    }

    #[test]
    fn test_iris_key_attestation_validates() -> Result<()> {
        let _ = color_eyre::install();
        check_attestation_validates(&Example {
            pubkey: ORB_IRIS_KEY,
            pubkey_is_der: false,
            extra_data: ORB_IRIS_KEY_EXTRA_DATA,
            sig: ORB_IRIS_KEY_SIG,
        })
    }
}
