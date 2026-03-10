use std::fmt::Debug;

use thiserror::Error;
use zerocopy::{big_endian, FromBytes, Immutable, KnownLayout, TryFromBytes};

#[derive(TryFromBytes, Immutable, KnownLayout, Debug, Eq, PartialEq, Clone, Copy)]
#[repr(u8)]
#[expect(non_camel_case_types)]
pub enum SecureObjectType {
    EC_KEY_PAIR = 0x01,
}

#[derive(
    FromBytes,
    Immutable,
    KnownLayout,
    Eq,
    PartialEq,
    Clone,
    Copy,
    derive_more::From,
    derive_more::Into,
)]
#[repr(transparent)]
pub struct ObjectId(pub big_endian::U32);

struct U32Formatter(big_endian::U32);

impl Debug for U32Formatter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#010X}", self.0)
    }
}

impl core::fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ObjectId")
            .field(&U32Formatter(self.0))
            .finish()
    }
}

impl core::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", U32Formatter(self.0))
    }
}

impl ObjectId {
    pub fn new(id: u32) -> Self {
        Self(id.into())
    }
}

impl PartialEq<u32> for ObjectId {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

#[derive(TryFromBytes, Immutable, KnownLayout, Debug, Eq, PartialEq, Clone, Copy)]
#[repr(u8)]
#[expect(non_camel_case_types)]
pub enum SetIndicator {
    NOT_SET = 0x01,
    SET = 0x02,
}

#[derive(TryFromBytes, KnownLayout, Immutable, Debug, PartialEq, Eq)]
#[repr(C, align(1))]
pub struct ObjectAttributes {
    object_identifier: ObjectId,
    object_class: SecureObjectType,
    authentication_indicator: SetIndicator,
    authentication_attempts_counter: big_endian::U16,
    authentication_object_identifier: ObjectId,
    maximum_authentication_attempts: big_endian::U16,
    policy_set: AttributesSuffix,
}

#[derive(TryFromBytes, Immutable, KnownLayout, Debug, Eq, PartialEq, Clone, Copy)]
#[repr(u8)]
#[expect(non_camel_case_types)]
pub enum Origin {
    ORIGIN_EXTERNAL = 0x01,
    ORIGIN_INTERNAL = 0x02,
    ORIGIN_PROVISIONED = 0x03,
}

#[derive(Debug, Error)]
#[error("failed to parse origin")]
pub struct OriginParseErr;

#[derive(TryFromBytes, KnownLayout, Immutable, Eq, PartialEq)]
#[repr(C)]
pub struct AttributesSuffix([u8]);

impl core::fmt::Debug for AttributesSuffix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct PolicySetFormatter<'a>(&'a AttributesSuffix);

        impl core::fmt::Debug for PolicySetFormatter<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let mut list = f.debug_list();
                for policy in self.0.policies() {
                    list.entry(&policy);
                }

                list.finish()
            }
        }

        f.debug_struct("AttributesSuffix")
            .field("origin", &self.origin())
            .field("policy_set", &PolicySetFormatter(self))
            .finish()
    }
}

impl AttributesSuffix {
    pub fn origin(&self) -> Result<Origin, OriginParseErr> {
        let v = self.0.last().unwrap();

        Origin::try_read_from_bytes(&[*v]).map_err(|_| OriginParseErr)
    }

    pub fn policies(&self) -> PolicySetIter<'_> {
        PolicySetIter {
            attributes_suffix: self,
            idx: 0,
        }
    }
}

pub struct PolicySetIter<'a> {
    attributes_suffix: &'a AttributesSuffix,
    idx: usize,
}

impl<'a> Iterator for PolicySetIter<'a> {
    type Item = &'a Policy;

    fn next(&mut self) -> Option<Self::Item> {
        // subtract 1 to account for origin suffix
        let policy_set_len = self.attributes_suffix.0.len() - 1;
        if self.idx >= policy_set_len {
            return None;
        }

        let policy_bytes = &self.attributes_suffix.0[self.idx..policy_set_len];
        let (header, _suffix) =
            zerocopy::Ref::<_, Policy>::from_prefix_with_elems(policy_bytes, 0)
                .unwrap();
        let (policy, remaining_bytes) =
            zerocopy::Ref::<_, Policy>::from_prefix_with_elems(
                policy_bytes,
                usize::from(
                    header.length_in_bytes
                        - (core::mem::size_of::<ObjectId>() as u8)
                        - 4, // length of access_rule header
                ),
            )
            .unwrap();

        self.idx = policy_set_len - remaining_bytes.len();

        Some(zerocopy::Ref::into_ref(policy))
    }
}

#[derive(FromBytes, KnownLayout, Immutable, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Policy {
    pub length_in_bytes: u8,
    pub authentication_object_id: ObjectId,
    pub access_rule: AccessRule,
}

#[derive(FromBytes, KnownLayout, Immutable, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct AccessRule {
    pub header: [u8; 4],
    pub extension: [u8],
}

pub const CHIP_ID_LEN: usize = 18;
pub const FRESHNESS_LEN: usize = 16;
pub const TIMESTAMP_LEN: usize = 12;

#[derive(Debug)]
pub struct ExtraData<'a> {
    pub object_attributes: &'a ObjectAttributes,
    pub timestamp: &'a [u8; TIMESTAMP_LEN],
    pub freshness: &'a [u8; FRESHNESS_LEN],
    pub chip_id: &'a [u8; CHIP_ID_LEN],
}

#[derive(Debug, Error)]
pub enum ParseExtraDataErr {
    #[error("the supplied bytes were too short to be valid")]
    TooShort,
    #[error("error during binary parsing: {0}")]
    ConvertError(#[from] ConvertErr),
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertErr {
    #[error("alignment")]
    Alignment,
    #[error("size")]
    Size,
    #[error("validity")]
    Validity,
}

impl<A, S, V> From<zerocopy::ConvertError<A, S, V>> for ConvertErr {
    fn from(value: zerocopy::ConvertError<A, S, V>) -> Self {
        use zerocopy::ConvertError;
        match value {
            ConvertError::Alignment(_) => Self::Alignment,
            ConvertError::Size(_) => Self::Size,
            ConvertError::Validity(_) => Self::Validity,
        }
    }
}

impl<'a> TryFrom<&'a [u8]> for ExtraData<'a> {
    type Error = ParseExtraDataErr;

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        let Some((obj_attrs, suffix)) =
            value.split_last_chunk::<{ TIMESTAMP_LEN + FRESHNESS_LEN + CHIP_ID_LEN }>()
        else {
            return Err(ParseExtraDataErr::TooShort);
        };

        let object_attributes = ObjectAttributes::try_ref_from_bytes(&obj_attrs)
            .map_err(ConvertErr::from)?;

        let (timestamp, suffix) = suffix
            .split_first_chunk::<TIMESTAMP_LEN>()
            .expect("infallible");

        let (freshness, suffix) = suffix
            .split_first_chunk::<FRESHNESS_LEN>()
            .expect("infallible");

        let chip_id: &[u8; CHIP_ID_LEN] = suffix.try_into().expect("infallible");

        Ok(Self {
            object_attributes,
            timestamp,
            freshness,
            chip_id,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const ORB_ATTESTATION_KEY: &[u8] =
        include_bytes!("../example_data/60000001.extra.raw");

    #[test]
    fn test_object_id_debug() {
        let obj = ObjectId::new(0x6000_0000);
        assert_eq!(format!("{obj:?}"), "ObjectId(0x60000000)");

        let obj = ObjectId::new(0x0000_6000);
        assert_eq!(format!("{obj:?}"), "ObjectId(0x00006000)");
    }

    #[test]
    fn test_object_id_display() {
        let obj = ObjectId::new(0x6000_0000);
        assert_eq!(format!("{obj}"), "0x60000000");

        let obj = ObjectId::new(0x0000_6000);
        assert_eq!(format!("{obj}"), "0x00006000");
    }

    #[test]
    fn test_orb_attestation_key_parses() {
        let suffix_len = FRESHNESS_LEN + TIMESTAMP_LEN + CHIP_ID_LEN;
        let obj_attrs = &ORB_ATTESTATION_KEY[..ORB_ATTESTATION_KEY.len() - suffix_len];

        assert_eq!(obj_attrs.len() + suffix_len, ORB_ATTESTATION_KEY.len());
        assert_eq!(suffix_len, 46);
        assert_eq!(obj_attrs.len(), 24);

        let foo = ObjectAttributes::try_ref_from_bytes(obj_attrs).unwrap();

        assert_eq!(foo.object_identifier, 0x60000001);
        assert_eq!(foo.object_class, SecureObjectType::EC_KEY_PAIR);
        assert_eq!(foo.authentication_indicator, SetIndicator::NOT_SET);
        assert_eq!(foo.authentication_attempts_counter, 0);
        assert_eq!(foo.authentication_object_identifier, 0x60000000);
        assert_eq!(foo.maximum_authentication_attempts, 0);

        let mut it = foo.policy_set.policies();

        let p1 = it.next().unwrap();
        assert_eq!(p1.length_in_bytes, 8);
        assert_eq!(p1.authentication_object_id, 0x60000000);
        assert_eq!(core::mem::size_of_val(&p1.access_rule), 4);

        assert!(it.next().is_none())
    }
}
