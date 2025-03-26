use orb_s3_helpers::S3Uri;
use std::{fmt::Display, path::PathBuf, str::FromStr};

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

impl OtaVersion {
    /// returns the version string without the ota:// prefix
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_s3_uri(&self) -> S3Uri {
        format!("s3://worldcoin-orb-updates-stage/{}/", self.as_str())
            .parse()
            .expect("this should be infallible")
    }
}

impl From<OtaVersion> for S3Uri {
    fn from(ota: OtaVersion) -> S3Uri {
        ota.to_s3_uri()
    }
}

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
        if suffix.contains('/') {
            return Err(OtaVersionParseErr::InvalidCharacter('/'));
        }

        Ok(Self(suffix.to_owned()))
    }
}

impl Display for OtaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("s3://")?;
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ota_into_s3() {
        let ota: OtaVersion = "ota://1.2.3".parse().expect("valid ota");
        let s3: S3Uri = "s3://worldcoin-orb-updates-stage/1.2.3/"
            .parse()
            .expect("valid s3");
        let converted = ota.to_s3_uri();
        assert_eq!(converted, ota.into());
        assert_eq!(converted, s3);
        assert!(converted.is_dir())
    }

    #[test]
    fn test_ota_into_s3_real_example() {
        let ota: OtaVersion = "ota://6.0.29+5d20de6.2410071904.dev"
            .parse()
            .expect("valid ota");
        let s3: S3Uri =
            "s3://worldcoin-orb-updates-stage/6.0.29+5d20de6.2410071904.dev/"
                .parse()
                .expect("valid s3");
        let converted = ota.to_s3_uri();
        assert_eq!(converted, ota.into());
        assert_eq!(converted, s3);
        assert!(converted.is_dir())
    }

    #[test]
    fn test_ota_version_display() {
        let version = OtaVersion("1.2.3".to_string());
        assert_eq!(version.to_string(), "s3://1.2.3");

        let version = OtaVersion("release-2023.01".to_string());
        assert_eq!(version.to_string(), "s3://release-2023.01");
    }

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
