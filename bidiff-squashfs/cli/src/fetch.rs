use orb_s3_helpers::S3Uri;
use std::{path::PathBuf, str::FromStr};

/// The different flavors of OTA identifiers that we support.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum OtaPath {
    S3(S3Uri),
    Version(OtaVersion),
    Path(PathBuf),
}

impl FromStr for OtaPath {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(s3) = s.parse::<S3Uri>() {
            return Ok(OtaPath::S3(s3));
        }
        if let Ok(ver) = s.parse::<OtaVersion>() {
            return Ok(OtaPath::Version(ver));
        }
        Ok(Self::Path(s.into()))
    }
}

#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Clone, Hash)]
pub struct OtaVersion(String);

#[derive(thiserror::Error, Debug)]
pub enum OtaVersionParseErr {
    #[error("wrong prefix: expected 'ota://'")]
    WrongPrefix,
    #[error("invalid character in OTA version: '{0}'")]
    InvalidCharacter(char),
}

impl FromStr for OtaVersion {
    type Err = OtaVersionParseErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some(suffix) = s.strip_prefix("ota://") else {
            return Err(OtaVersionParseErr::WrongPrefix);
        };
        if s.contains('/') {
            return Err(OtaVersionParseErr::InvalidCharacter('/'));
        }

        Ok(Self(suffix.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_ota_version() {
        let version = "ota://1.2.3".parse::<OtaVersion>().unwrap();
        assert_eq!(version.0, "1.2.3");

        let version = "ota://release-2023.01".parse::<OtaVersion>().unwrap();
        assert_eq!(version.0, "release-2023.01");
    }

    #[test]
    fn test_missing_prefix() {
        let err = "1.2.3".parse::<OtaVersion>().unwrap_err();
        assert!(matches!(err, OtaVersionParseErr::WrongPrefix));

        let err = "http://1.2.3".parse::<OtaVersion>().unwrap_err();
        assert!(matches!(err, OtaVersionParseErr::WrongPrefix));
    }

    #[test]
    fn test_invalid_characters() {
        let err = "ota://1.2.3/extra".parse::<OtaVersion>().unwrap_err();
        assert!(matches!(err, OtaVersionParseErr::InvalidCharacter('/')));
    }
}
