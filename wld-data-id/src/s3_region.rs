//! A representation of a AWS S3 Region used in WLD Data IDs.

use eyre::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{convert::Infallible, str::FromStr};

#[allow(missing_docs)]
#[derive(JsonSchema, Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum S3Region {
    AfSouth1 = 0,
    ApEast1 = 1,
    ApNortheast1 = 2,
    ApNortheast2 = 3,
    ApNortheast3 = 4,
    ApSouth1 = 5,
    ApSoutheast1 = 6,
    ApSoutheast2 = 7,
    CaCentral1 = 8,
    CnNorthwest1 = 9,
    EuCentral1 = 10,
    EuNorth1 = 11,
    EuSouth1 = 12,
    EuWest1 = 13,
    EuWest2 = 14,
    EuWest3 = 15,
    MeSouth1 = 16,
    SaEast1 = 17,
    UsEast1 = 18,
    UsEast2 = 19,
    UsGovEast1 = 20,
    UsGovWest1 = 21,
    UsWest1 = 22,
    UsWest2 = 23,
    #[default]
    Unknown = 0xFF,
}

impl FromStr for S3Region {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "af-south-1" => S3Region::AfSouth1,
            "ap-east-1" => S3Region::ApEast1,
            "ap-northeast-1" => S3Region::ApNortheast1,
            "ap-northeast-2" => S3Region::ApNortheast2,
            "ap-northeast-3" => S3Region::ApNortheast3,
            "ap-south-1" => S3Region::ApSouth1,
            "ap-southeast-1" => S3Region::ApSoutheast1,
            "ap-southeast-2" => S3Region::ApSoutheast2,
            "ca-central-1" => S3Region::CaCentral1,
            "cn-northwest-1" => S3Region::CnNorthwest1,
            "eu-central-1" => S3Region::EuCentral1,
            "eu-north-1" => S3Region::EuNorth1,
            "eu-south-1" => S3Region::EuSouth1,
            "eu-west-1" => S3Region::EuWest1,
            "eu-west-2" => S3Region::EuWest2,
            "eu-west-3" => S3Region::EuWest3,
            "me-south-1" => S3Region::MeSouth1,
            "sa-east-1" => S3Region::SaEast1,
            "us-east-1" => S3Region::UsEast1,
            "us-east-2" => S3Region::UsEast2,
            "us-gov-east-1" => S3Region::UsGovEast1,
            "us-gov-west-1" => S3Region::UsGovWest1,
            "us-west-1" => S3Region::UsWest1,
            "us-west-2" => S3Region::UsWest2,
            _ => S3Region::Unknown,
        })
    }
}

impl Serialize for S3Region {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (*self as u8).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for S3Region {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value: u8 = Deserialize::deserialize(deserializer)?;
        // TODO: Consider using a derive macro like https://crates.io/crates/enum-primitive-derive instead
        Ok(match value {
            0 => S3Region::AfSouth1,
            1 => S3Region::ApEast1,
            2 => S3Region::ApNortheast1,
            3 => S3Region::ApNortheast2,
            4 => S3Region::ApNortheast3,
            5 => S3Region::ApSouth1,
            6 => S3Region::ApSoutheast1,
            7 => S3Region::ApSoutheast2,
            8 => S3Region::CaCentral1,
            9 => S3Region::CnNorthwest1,
            10 => S3Region::EuCentral1,
            11 => S3Region::EuNorth1,
            12 => S3Region::EuSouth1,
            13 => S3Region::EuWest1,
            14 => S3Region::EuWest2,
            15 => S3Region::EuWest3,
            16 => S3Region::MeSouth1,
            17 => S3Region::SaEast1,
            18 => S3Region::UsEast1,
            19 => S3Region::UsEast2,
            20 => S3Region::UsGovEast1,
            21 => S3Region::UsGovWest1,
            22 => S3Region::UsWest1,
            23 => S3Region::UsWest2,
            _ => S3Region::Unknown,
        })
    }
}
