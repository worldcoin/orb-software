use bon::Builder;
use der::Decode;
use zerocopy::IntoBytes;

use p256::ecdsa::{signature::Verifier as _, Signature as P256Signature};

use crate::{
    attributes::{
        ObjectAttributes, ObjectId, Origin, OriginParseErr, SecureObjectType,
        SetIndicator,
    },
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
pub enum AttributesErr {
    #[error("wrong object class: expected {expected:?}, got {encountered:?}")]
    WrongObjectClass {
        expected: SecureObjectType,
        encountered: SecureObjectType,
    },
    #[error(
        "wrong authentication indicator: expected {expected:?}, got {encountered:?}"
    )]
    WrongAuthenticationIndicator {
        expected: SetIndicator,
        encountered: SetIndicator,
    },
    #[error("wrong object id: expected {expected:?}, got {encountered:?}")]
    WrongObjectId {
        expected: ObjectId,
        encountered: ObjectId,
    },
    #[error("exceeded max allowed auth attempts: {failed} >= {max}")]
    ExceededMaxAuthAttempts { max: u16, failed: u16 },
    #[error("wrong authentication object: expected {expected:?}, got {encountered:?}")]
    WrongAuthenticationObject {
        expected: ObjectId,
        encountered: ObjectId,
    },
    #[error("wrong origin: expected {expected:?}, got {encountered:?}")]
    WrongOrigin {
        expected: Origin,
        encountered: Origin,
    },
    #[error(transparent)]
    OriginParse(#[from] OriginParseErr),
}

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
    #[error("cert chip id was {cert} but attestation chip id was {attestation}")]
    ChipIdMismatch { attestation: ChipId, cert: ChipId },
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

    fn validate_attrs(&self, key_type: OrbKeyType) -> Result<(), AttributesErr> {
        if key_type != self.attrs.object_identifier {
            return Err(AttributesErr::WrongObjectId {
                expected: key_type.into(),
                encountered: self.attrs.object_identifier,
            });
        }

        let expected_object_class = match key_type {
            OrbKeyType::Session => SecureObjectType::EC_PUB_KEY,
            OrbKeyType::Attestation | OrbKeyType::Iris => SecureObjectType::EC_KEY_PAIR,
        };
        if self.attrs.object_class != expected_object_class {
            return Err(AttributesErr::WrongObjectClass {
                expected: expected_object_class,
                encountered: self.attrs.object_class,
            });
        }

        let expected_authentication_indicator = match key_type {
            OrbKeyType::Session => SetIndicator::SET,
            OrbKeyType::Attestation | OrbKeyType::Iris => SetIndicator::NOT_SET,
        };
        if self.attrs.authentication_indicator != expected_authentication_indicator {
            return Err(AttributesErr::WrongAuthenticationIndicator {
                expected: expected_authentication_indicator,
                encountered: self.attrs.authentication_indicator,
            });
        }

        if self.attrs.maximum_authentication_attempts > 0
            && self.attrs.authentication_attempts_counter
                >= self.attrs.maximum_authentication_attempts
        {
            return Err(AttributesErr::ExceededMaxAuthAttempts {
                max: self.attrs.maximum_authentication_attempts.get(),
                failed: self.attrs.authentication_attempts_counter.get(),
            });
        }

        let expected_auth_object = match key_type {
            OrbKeyType::Session => ObjectId::new(0),
            OrbKeyType::Attestation | OrbKeyType::Iris => OrbKeyType::Session.into(),
        };
        if self.attrs.authentication_object_identifier != expected_auth_object {
            return Err(AttributesErr::WrongAuthenticationObject {
                expected: OrbKeyType::Session.into(),
                encountered: self.attrs.authentication_object_identifier,
            });
        }

        let expected_origin = match key_type {
            OrbKeyType::Session => Origin::ORIGIN_EXTERNAL,
            OrbKeyType::Attestation | OrbKeyType::Iris => Origin::ORIGIN_INTERNAL,
        };
        let origin = self.attrs.policy_set.origin()?;
        if origin != expected_origin {
            return Err(AttributesErr::WrongOrigin {
                expected: expected_origin,
                encountered: origin,
            });
        }

        Ok(())
    }

    fn validate(
        self,
        key_type: OrbKeyType,
        chip_unique_pubkey: &ChipUniquePubkey,
    ) -> Result<ValidatedAttestation<'a>, ValidateErr> {
        self.validate_sig(chip_unique_pubkey)?;
        self.validate_attrs(key_type)?;

        Ok(ValidatedAttestation(self))
    }

    pub fn validate_from_cert(
        self,
        key_type: OrbKeyType,
        chip_cert_pem: &str,
        current_time: rustls_pki_types::UnixTime,
    ) -> Result<ValidatedAttestation<'a>, ValidateFromCertErr> {
        let (chip_unique_pubkey, chip_id) =
            crate::certs::verify_cert(chip_cert_pem, current_time)?;

        if chip_id != self.chip_id {
            return Err(ValidateFromCertErr::ChipIdMismatch {
                attestation: self.chip_id,
                cert: chip_id,
            });
        }
        Ok(self.validate(key_type, &chip_unique_pubkey)?)
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u32)]
pub enum OrbKeyType {
    Session = 0x60000000,
    Attestation = 0x60000001,
    Iris = 0x60000002,
}

impl PartialEq<ObjectId> for OrbKeyType {
    fn eq(&self, other: &ObjectId) -> bool {
        (*self as u32) == other.0.get()
    }
}

impl From<OrbKeyType> for ObjectId {
    fn from(value: OrbKeyType) -> Self {
        Self::new(value as _)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("encountered unknown object id {0}")]
pub struct UnknownObjectIdErr(ObjectId);

impl TryFrom<ObjectId> for OrbKeyType {
    type Error = UnknownObjectIdErr;

    fn try_from(value: ObjectId) -> Result<Self, Self::Error> {
        Ok(match value {
            val if Self::Session == val => Self::Session,
            val if Self::Attestation == val => Self::Attestation,
            val if Self::Iris == val => Self::Iris,
            _ => return Err(UnknownObjectIdErr(value)),
        })
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

    use color_eyre::eyre::{ensure, Context as _};
    use color_eyre::Result;
    use rustls_pki_types::UnixTime;

    struct Example {
        pubkey: &'static [u8],
        pubkey_is_der: bool,
        extra_data: &'static [u8],
        sig: &'static [u8],
        key_type: OrbKeyType,
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
            .validate_from_cert(
                example.key_type,
                crate::example_data::CERT,
                valid_cert_time,
            )
            .wrap_err("attestion should validate")?;

        Ok(())
    }

    #[test]
    fn test_session_key_attestation_validates() -> Result<()> {
        let _ = color_eyre::install();
        let example = Example {
            pubkey: ORB_SESSION_KEY,
            pubkey_is_der: true,
            extra_data: ORB_SESSION_KEY_EXTRA_DATA,
            sig: ORB_SESSION_KEY_SIG,
            key_type: OrbKeyType::Session,
        };
        check_attestation_validates(&example)?;

        ensure!(check_attestation_validates(&Example {
            key_type: OrbKeyType::Attestation,
            ..example
        })
        .is_err());
        ensure!(check_attestation_validates(&Example {
            key_type: OrbKeyType::Iris,
            ..example
        })
        .is_err());

        Ok(())
    }

    #[test]
    fn test_attestation_key_attestation_validates() -> Result<()> {
        let _ = color_eyre::install();
        let example = Example {
            pubkey: ORB_ATTESTATION_KEY,
            pubkey_is_der: false,
            extra_data: ORB_ATTESTATION_KEY_EXTRA_DATA,
            sig: ORB_ATTESTATION_KEY_SIG,
            key_type: OrbKeyType::Attestation,
        };
        check_attestation_validates(&example)?;

        ensure!(check_attestation_validates(&Example {
            key_type: OrbKeyType::Session,
            ..example
        })
        .is_err());
        ensure!(check_attestation_validates(&Example {
            key_type: OrbKeyType::Iris,
            ..example
        })
        .is_err());

        Ok(())
    }

    #[test]
    fn test_iris_key_attestation_validates() -> Result<()> {
        let _ = color_eyre::install();
        let example = Example {
            pubkey: ORB_IRIS_KEY,
            pubkey_is_der: false,
            extra_data: ORB_IRIS_KEY_EXTRA_DATA,
            sig: ORB_IRIS_KEY_SIG,
            key_type: OrbKeyType::Iris,
        };
        check_attestation_validates(&example)?;
        ensure!(check_attestation_validates(&Example {
            key_type: OrbKeyType::Session,
            ..example
        })
        .is_err());
        ensure!(check_attestation_validates(&Example {
            key_type: OrbKeyType::Attestation,
            ..example
        })
        .is_err());

        Ok(())
    }
}
