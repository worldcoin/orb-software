use derive_more::Display;
use std::collections::HashMap;
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

    #[error("Unknown platform type: {0}")]
    UnknownPlatformType(String),
}

#[derive(Display, Debug, Clone, PartialEq, Eq, Copy)]
pub enum OrbOsPlatform {
    #[display("diamond")]
    Diamond,

    #[display("pearl")]
    Pearl,
}

#[derive(Display, Debug, Copy, Clone, PartialEq, Eq)]
pub enum OrbRelease {
    #[display("dev")]
    Dev,
    #[display("service")]
    Service,
    #[display("prod")]
    Prod,
    #[display("stage")]
    Stage,
    #[display("analysis")]
    Analysis,
}

impl OrbRelease {
    pub fn as_str(&self) -> &str {
        use OrbRelease::*;
        match self {
            Dev => "dev",
            Service => "service",
            Staging => "staging",
            Prod => "prod",
            Analysis => "analysis",
        }
    }
}

#[derive(Display, Debug, Clone)]
#[display("ORB_OS_RELEASE_TYPE={release_type}\nORB_OS_PLATFORM_TYPE={orb_os_platform_type}\nORB_OS_EXPECTED_MAIN_MCU_VERSION={expected_main_mcu_version}\nORB_OS_EXPECTED_SEC_MCU_VERSION={expected_sec_mcu_version}")]
pub struct OrbOsRelease {
    pub release_type: OrbRelease,
    pub orb_os_platform_type: OrbOsPlatform,
    pub expected_main_mcu_version: String,
    pub expected_sec_mcu_version: String,
}

impl OrbOsRelease {
    pub fn parse(file_contents: String) -> Result<Self, ReadErr> {
        let map: HashMap<String, String> = file_contents
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if let Some((k, v)) = line.split_once('=') {
                    let v = v.trim_matches('"');
                    Some((k.trim().to_string(), v.to_string()))
                } else {
                    None
                }
            })
            .collect();

        let release_type = match map.get("ORB_OS_RELEASE_TYPE").map(|s| s.as_str()) {
            Some("dev") => OrbRelease::Dev,
            Some("service") => OrbRelease::Service,
            Some("prod") => OrbRelease::Prod,
            Some("stage") => OrbRelease::Stage,
            Some("analysis") => OrbRelease::Analysis,
            Some(other) => return Err(ReadErr::UnknownReleaseType(other.to_string())),
            None => return Err(ReadErr::MissingField("ORB_OS_RELEASE_TYPE")),
        };

        let orb_os_platform_type = match map
            .get("ORB_OS_PLATFORM_TYPE")
            .map(|s| s.as_str())
        {
            Some("diamond") => OrbOsPlatform::Diamond,
            Some("pearl") => OrbOsPlatform::Pearl,
            Some(other) => return Err(ReadErr::UnknownPlatformType(other.to_string())),
            None => return Err(ReadErr::MissingField("ORB_OS_PLATFORM_TYPE")),
        };

        let expected_main_mcu_version = map
            .get("ORB_OS_EXPECTED_MAIN_MCU_VERSION")
            .cloned()
            .ok_or(ReadErr::MissingField("ORB_OS_EXPECTED_MAIN_MCU_VERSION"))?;

        let expected_sec_mcu_version = map
            .get("ORB_OS_EXPECTED_SEC_MCU_VERSION")
            .cloned()
            .ok_or(ReadErr::MissingField("ORB_OS_EXPECTED_SEC_MCU_VERSION"))?;

        Ok(Self {
            release_type,
            orb_os_platform_type,
            expected_main_mcu_version,
            expected_sec_mcu_version,
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
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15
        ORB_OS_PLATFORM_TYPE=diamond"#;

        let os_release = OrbOsRelease::parse(os_release_content.to_string()).unwrap();
        println!("{os_release}");

        assert_eq!(os_release.release_type, OrbRelease::Dev);
        assert_eq!(os_release.orb_os_platform_type, OrbOsPlatform::Diamond);
        assert_eq!(os_release.expected_main_mcu_version, "v3.0.15");
        assert_eq!(os_release.expected_sec_mcu_version, "v3.0.15");
    }
    #[test]
    fn test_parse_missing_release_type() {
        let broken_input = r#"ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
        ORB_OS_PLATFORM_TYPE=pearl
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;

        let result = OrbOsRelease::parse(broken_input.to_string());

        assert!(matches!(
            result,
            Err(ReadErr::MissingField("ORB_OS_RELEASE_TYPE"))
        ));
    }

    #[test]
    fn test_missing_platform_type() {
        let broken_input = r#"ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
        ORB_OS_RELEASE_TYPE=dev
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;

        let result = OrbOsRelease::parse(broken_input.to_string());

        assert!(matches!(
            result,
            Err(ReadErr::MissingField("ORB_OS_PLATFORM_TYPE"))
        ));
    }

    #[test]
    fn test_parse_missing_main_mcu_version() {
        let broken_input = r#"ORB_OS_RELEASE_TYPE=dev
        ORB_OS_PLATFORM_TYPE=pearl
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
        ORB_OS_PLATFORM_TYPE=pearl
        ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;

        let result = OrbOsRelease::parse(broken_input.to_string());

        assert!(
            matches!(result, Err(ReadErr::UnknownReleaseType(ref s)) if s == "unknown")
        );
    }

    #[test]
    fn test_parse_invalid_platform_type() {
        let broken_input = r#"ORB_OS_RELEASE_TYPE=dev
        ORB_OS_PLATFORM_TYPE=unknown
        ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
        ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;

        let result = OrbOsRelease::parse(broken_input.to_string());

        assert!(
            matches!(result, Err(ReadErr::UnknownPlatformType(ref s)) if s == "unknown")
        );
    }
}
