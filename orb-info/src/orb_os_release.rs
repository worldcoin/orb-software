use std::fmt;
use thiserror::Error;

use crate::from_file_blocking;

const ORB_OS_RELEASE_PATH: &str = "/etc/os-release";

#[derive(Debug, Error)]
pub enum ReadErr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Missing or malformed expected value: {0}")]
    MissingField(&'static str),

    #[error("Unknown release type: {0}")]
    UnknownReleaseType(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrbReleaseType {
    Dev,
    Service,
    Prod,
}

impl fmt::Display for OrbReleaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            OrbReleaseType::Dev => "dev",
            OrbReleaseType::Service => "service",
            OrbReleaseType::Prod => "prod",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone)]
pub struct OrbOsRelease {
    pub release_type: OrbReleaseType,
    pub expected_main_mcu_version: String,
    pub expected_sec_mcu_version: String,
}

impl fmt::Display for OrbOsRelease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ORB_OS_RELEASE_TYPE={}\nORB_OS_EXPECTED_MAIN_MCU_VERSION={}\nORB_OS_EXPECTED_SEC_MCU_VERSION={}",
            self.release_type,
            self.expected_main_mcu_version,
            self.expected_sec_mcu_version
        )
    }
}

impl OrbOsRelease {
    fn parse(file_contents: String) -> Result<Self, ReadErr> {
        let mut release_type = None;
        let mut expected_main_mcu_version = None;
        let mut expected_sec_mcu_version = None;

        for line in file_contents.lines().rev() {
            if let Some((_, data)) = line.split_once("ORB_OS_EXPECTED_SEC_MCU_VERSION=")
            {
                expected_sec_mcu_version = Some(data.to_string());
            } else if let Some((_, data)) =
                line.split_once("ORB_OS_EXPECTED_MAIN_MCU_VERSION=")
            {
                expected_main_mcu_version = Some(data.to_string());
            } else if let Some((_, data)) = line.split_once("ORB_OS_RELEASE_TYPE=") {
                release_type = Some(match data.to_lowercase().as_str() {
                    "dev" => OrbReleaseType::Dev,
                    "service" => OrbReleaseType::Service,
                    "prod" => OrbReleaseType::Prod,
                    other => {
                        return Err(ReadErr::UnknownReleaseType(other.to_string()))
                    }
                });
            }
        }

        Ok(Self {
            release_type: release_type
                .ok_or(ReadErr::MissingField("ORB_OS_RELEASE_TYPE"))?,
            expected_main_mcu_version: expected_main_mcu_version
                .ok_or(ReadErr::MissingField("ORB_OS_EXPECTED_MAIN_MCU_VERSION"))?,
            expected_sec_mcu_version: expected_sec_mcu_version
                .ok_or(ReadErr::MissingField("ORB_OS_EXPECTED_SEC_MCU_VERSION"))?,
        })
    }

    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, ReadErr> {
        use crate::from_file;
        let file_contents = from_file(ORB_OS_RELEASE_PATH).await?;

        OrbOsRelease::parse(file_contents)
    }

    pub fn read_blocking() -> Result<Self, ReadErr> {
        let file_contents = from_file_blocking(ORB_OS_RELEASE_PATH)?;
        OrbOsRelease::parse(file_contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_from_valid_string() {
        let os_release_content: &str = r#"PRETTY_NAME="Ubuntu 22.04.5 LTS"
        NAME="Ubuntu"
        VERSION_ID="22.04"
        VERSION="22.04.5 LTS (Jammy Jellyfish)"
        VERSION_CODENAME=jammy
        ID=ubuntu
        ID_LIKE=debian
        HOME_URL="https://www.ubuntu.com/"
        SUPPORT_URL="https://help.ubuntu.com/"
        BUG_REPORT_URL="https://bugs.launchpad.net/ubuntu/"
        PRIVACY_POLICY_URL="https://www.ubuntu.com/legal/terms-and-policies/privacy-policy"
        UBUNTU_CODENAME=jammy
        ORB_OS_RELEASE_TYPE=dev
        ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;

        let os_release = OrbOsRelease::parse(os_release_content.to_string()).unwrap();

        assert_eq!(os_release.release_type, OrbReleaseType::Dev);
        assert_eq!(os_release.expected_main_mcu_version, "v3.0.15");
        assert_eq!(os_release.expected_sec_mcu_version, "v3.0.15");
    }
    #[test]
    fn test_parse_missing_release_type() {
        let broken_input = r#"ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;

        let result = OrbOsRelease::parse(broken_input.to_string());

        assert!(matches!(
            result,
            Err(ReadErr::MissingField("ORB_OS_RELEASE_TYPE"))
        ));
    }

    #[test]
    fn test_parse_missing_main_mcu_version() {
        let broken_input = r#"ORB_OS_RELEASE_TYPE=dev
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;

        let result = OrbOsRelease::parse(broken_input.to_string());

        assert!(matches!(
            result,
            Err(ReadErr::MissingField("ORB_OS_EXPECTED_MAIN_MCU_VERSION"))
        ));
    }

    #[test]
    fn test_parse_invalid_release_type() {
        let broken_input = r#"ORB_OS_RELEASE_TYPE=unknown
        ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;

        let result = OrbOsRelease::parse(broken_input.to_string());

        assert!(
            matches!(result, Err(ReadErr::UnknownReleaseType(ref s)) if s == "unknown")
        );
    }
}
