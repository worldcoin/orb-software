use std::{fmt::Debug, ops::Deref};

use thiserror::Error;
use zerocopy::{
    big_endian, FromBytes, Immutable, IntoBytes, KnownLayout, TryFromBytes,
};

/// See section 4.3.6 of AN12413
#[derive(
    TryFromBytes, IntoBytes, Immutable, KnownLayout, Debug, Eq, PartialEq, Clone, Copy,
)]
#[repr(u8)]
#[expect(non_camel_case_types)]
pub enum SecureObjectType {
    EC_KEY_PAIR = 0x01,
    EC_PRIV_KEY = 0x02,
    EC_PUB_KEY = 0x03,
}

#[derive(
    FromBytes,
    IntoBytes,
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

/// See section 4.3.29 of AN12413
#[derive(
    TryFromBytes, IntoBytes, Immutable, KnownLayout, Debug, Eq, PartialEq, Clone, Copy,
)]
#[repr(u8)]
#[expect(non_camel_case_types)]
pub enum SetIndicator {
    NOT_SET = 0x01,
    SET = 0x02,
}

#[derive(TryFromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq, Eq)]
#[repr(C, align(1))]
pub struct ObjectAttrsHeader {
    pub object_identifier: ObjectId,
    pub object_class: SecureObjectType,
    pub authentication_indicator: SetIndicator,
    pub authentication_attempts_counter: big_endian::U16,
    pub authentication_object_identifier: ObjectId,
    pub maximum_authentication_attempts: big_endian::U16,
}

#[derive(TryFromBytes, KnownLayout, Immutable, Debug, PartialEq, Eq)]
#[repr(C, align(1))]
pub struct ObjectAttributes {
    header: ObjectAttrsHeader,
    pub policy_set: AttributesSuffix,
}

impl Deref for ObjectAttributes {
    type Target = ObjectAttrsHeader;

    fn deref(&self) -> &Self::Target {
        &self.header
    }
}

/// See section 4.3.8 of AN12413
#[derive(
    TryFromBytes, IntoBytes, Immutable, KnownLayout, Debug, Eq, PartialEq, Clone, Copy,
)]
#[repr(u8)]
#[expect(non_camel_case_types)]
pub enum Origin {
    ORIGIN_EXTERNAL = 0x01,
    ORIGIN_INTERNAL = 0x02,
    ORIGIN_PROVISIONED = 0x03,
}

#[derive(Debug, Error, Eq, PartialEq)]
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

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct AccessRule {
    pub header: [u8; 4],
    pub extension: [u8],
}

#[cfg(test)]
mod test {
    use crate::{
        example_data::{ORB_ATTESTATION_KEY, ORB_IRIS_KEY, ORB_SESSION_KEY},
        extra_data::ExtraData,
    };

    use super::*;

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
    fn test_orb_session_key_parses() {
        let extra_data =
            ExtraData::try_from(ORB_SESSION_KEY).expect("failed to parse extradata");
        let attrs = extra_data.object_attributes;

        assert_eq!(attrs.object_identifier, 0x60000000);
        assert_eq!(attrs.object_class, SecureObjectType::EC_PUB_KEY);
        assert_eq!(attrs.authentication_indicator, SetIndicator::SET);
        assert_eq!(attrs.authentication_attempts_counter, 0);
        assert_eq!(attrs.authentication_object_identifier, 0x00000000);
        assert_eq!(attrs.maximum_authentication_attempts, 0);
        assert_eq!(attrs.policy_set.origin(), Ok(Origin::ORIGIN_EXTERNAL));

        let mut it = attrs.policy_set.policies();

        assert!(it.next().is_none());
    }

    #[test]
    fn test_orb_attestation_key_parses() {
        let extra_data = ExtraData::try_from(ORB_ATTESTATION_KEY)
            .expect("failed to parse extradata");
        let attrs = extra_data.object_attributes;

        assert_eq!(attrs.object_identifier, 0x60000001);
        assert_eq!(attrs.object_class, SecureObjectType::EC_KEY_PAIR);
        assert_eq!(attrs.authentication_indicator, SetIndicator::NOT_SET);
        assert_eq!(attrs.authentication_attempts_counter, 0);
        assert_eq!(attrs.authentication_object_identifier, 0x60000000);
        assert_eq!(attrs.maximum_authentication_attempts, 0);
        assert_eq!(attrs.policy_set.origin(), Ok(Origin::ORIGIN_INTERNAL));

        let mut it = attrs.policy_set.policies();

        let p1 = it.next().unwrap();
        assert_eq!(p1.length_in_bytes, 8);
        assert_eq!(p1.authentication_object_id, 0x60000000);
        assert_eq!(core::mem::size_of_val(&p1.access_rule), 4);

        assert!(it.next().is_none())
    }

    #[test]
    fn test_orb_iris_key_parses() {
        let extra_data =
            ExtraData::try_from(ORB_IRIS_KEY).expect("failed to parse extradata");
        let attrs = extra_data.object_attributes;

        assert_eq!(attrs.object_identifier, 0x60000002);
        assert_eq!(attrs.object_class, SecureObjectType::EC_KEY_PAIR);
        assert_eq!(attrs.authentication_indicator, SetIndicator::NOT_SET);
        assert_eq!(attrs.authentication_attempts_counter, 0);
        assert_eq!(attrs.authentication_object_identifier, 0x60000000);
        assert_eq!(attrs.maximum_authentication_attempts, 0);
        assert_eq!(attrs.policy_set.origin(), Ok(Origin::ORIGIN_INTERNAL));

        let mut it = attrs.policy_set.policies();

        let p1 = it.next().unwrap();
        assert_eq!(p1.length_in_bytes, 8);
        assert_eq!(p1.authentication_object_id, 0x60000000);
        assert_eq!(core::mem::size_of_val(&p1.access_rule), 4);

        assert!(it.next().is_none())
    }
}
