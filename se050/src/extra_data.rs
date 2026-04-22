use thiserror::Error;
use zerocopy::TryFromBytes as _;

use crate::attributes::ObjectAttributes;

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

        let object_attributes = ObjectAttributes::try_ref_from_bytes(obj_attrs)
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
    use hex_literal::hex;

    use crate::example_data::{ORB_ATTESTATION_KEY, ORB_IRIS_KEY, ORB_SESSION_KEY};

    use super::*;

    fn check_attrs_len(bytes: &[u8], expected_len: usize) -> ExtraData<'_> {
        let suffix_len = FRESHNESS_LEN + TIMESTAMP_LEN + CHIP_ID_LEN;
        assert_eq!(suffix_len, 46, "sanity");

        let attrs_len = bytes.len() - suffix_len;
        assert_eq!(attrs_len, expected_len);

        ExtraData::try_from(bytes).expect("failed to parse ExtraData")
    }

    #[test]
    fn test_orb_session_key_parses() {
        let timestamp = &hex!("0000 002c 0000 0000 0018 98e0");
        let freshness = &hex!("8c56 ac55 c9bd e3b4 1aeb c3c7 002e b034");
        let chip_id = &hex!("0400 5001 94b5 8d02 eab2 9b04 6aa2 6a70 1b90");

        let ed = check_attrs_len(ORB_SESSION_KEY, 15);
        assert_eq!(ed.timestamp, timestamp);
        assert_eq!(ed.freshness, freshness);
        assert_eq!(ed.chip_id, chip_id);
    }

    #[test]
    fn test_orb_attestation_key_parses() {
        let timestamp = &hex!("0000 0028 0000 0000 001b 6f70");
        let freshness = &hex!("e833 7b03 a3ce 4d9b 5d69 8846 17dd 54bf");
        let chip_id = &hex!("0400 5001 94b5 8d02 eab2 9b04 6aa2 6a70 1b90");

        let ed = check_attrs_len(ORB_ATTESTATION_KEY, 24);
        assert_eq!(ed.timestamp, timestamp);
        assert_eq!(ed.freshness, freshness);
        assert_eq!(ed.chip_id, chip_id);
    }

    #[test]
    fn test_orb_iris_key_parses() {
        let timestamp = &hex!("0000 002a 0000 0000 001b 6f70");
        let freshness = &hex!("71a9 8ee8 7851 f2b5 9e6e 6c98 7dab 5e34");
        let chip_id = &hex!("0400 5001 94b5 8d02 eab2 9b04 6aa2 6a70 1b90");

        let ed = check_attrs_len(ORB_IRIS_KEY, 24);
        assert_eq!(ed.timestamp, timestamp);
        assert_eq!(ed.freshness, freshness);
        assert_eq!(ed.chip_id, chip_id);
    }
}
