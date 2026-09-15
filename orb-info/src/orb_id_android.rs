#[cfg(feature = "async")]
use crate::from_file;
use crate::from_file_blocking;
#[cfg(feature = "async")]
use futures::TryFutureExt;
#[cfg(feature = "async")]
use std::future;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OrbId(pub(crate) u32);

// A suffix to append to orb-id to distinguish it from Linux based orbs. Use suffix
// and not prefix because backend like to truncate orb id to 8 characters, thus
// loosing the actual orb id
const SUFFIX: &str = "arkenstone";

impl std::fmt::Display for OrbId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{SUFFIX}", self.0)
    }
}

impl std::str::FromStr for OrbId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.strip_suffix(SUFFIX).unwrap_or(s).parse()?))
    }
}

// Serialize/deserialize as a string via Display/FromStr, so the `arkenstone`
// suffix is included on the wire.
#[cfg(feature = "serde")]
impl serde::Serialize for OrbId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for OrbId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadErr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] std::num::ParseIntError),
}

#[cfg(not(test))]
const SOC_SERIAL_NUMBER_PATH: &str = "/sys/devices/soc0/serial_number";
#[cfg(test)]
const SOC_SERIAL_NUMBER_PATH: &str = "./test_soc_serial_number";

impl OrbId {
    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, ReadErr> {
        let s = future::ready(std::env::var("ORB_ID"))
            .map_ok(|s| s.to_string())
            .or_else(|_| from_file(SOC_SERIAL_NUMBER_PATH))
            .await?;

        Ok(s.parse()?)
    }

    pub fn read_blocking() -> Result<Self, ReadErr> {
        let s = std::env::var("ORB_ID")
            .map(|s| s.to_string())
            .or_else(|_| from_file_blocking(SOC_SERIAL_NUMBER_PATH))?;

        Ok(s.parse()?)
    }
}

#[cfg(any(test, feature = "testing"))]
pub fn test_orb_id() -> OrbId {
    "666666".parse().unwrap()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_from_str_and_display() {
        let id: OrbId = "1234".parse().unwrap();
        assert_eq!(id.0, 1234);
        assert_eq!(id.to_string(), "1234arkenstone");
    }

    #[test]
    fn test_from_str_accepts_suffixed() {
        let id: OrbId = "1234arkenstone".parse().unwrap();
        assert_eq!(id.0, 1234);
    }

    #[test]
    #[serial_test::serial]
    fn test_sync_get_orb_id_from_env() {
        std::env::set_var("ORB_ID", "1234");

        let orb_id = OrbId::read_blocking().unwrap();
        assert_eq!(orb_id.0, 1234);

        std::env::remove_var("ORB_ID");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial_test::serial]
    async fn test_async_get_orb_id_from_env() {
        std::env::set_var("ORB_ID", "1234");

        let orb_id = OrbId::read().await.unwrap();
        assert_eq!(orb_id.0, 1234);

        std::env::remove_var("ORB_ID");
    }

    #[test]
    #[serial_test::serial]
    fn test_sync_get_orb_id_from_file() {
        std::env::remove_var("ORB_ID");
        std::fs::write(SOC_SERIAL_NUMBER_PATH, "5678\n").unwrap();

        let orb_id = OrbId::read_blocking().unwrap();
        assert_eq!(orb_id.0, 5678);

        std::fs::remove_file(SOC_SERIAL_NUMBER_PATH).unwrap();
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial_test::serial]
    async fn test_async_get_orb_id_from_file() {
        std::env::remove_var("ORB_ID");
        tokio::fs::write(SOC_SERIAL_NUMBER_PATH, "5678\n")
            .await
            .unwrap();

        let orb_id = OrbId::read().await.unwrap();
        assert_eq!(orb_id.0, 5678);

        tokio::fs::remove_file(SOC_SERIAL_NUMBER_PATH)
            .await
            .unwrap();
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_orb_id() {
        let json = serde_json::json!("1234");
        let id: OrbId = serde_json::from_value(json).unwrap();
        assert_eq!(id.0, 1234);
        assert_eq!(
            serde_json::to_value(id).unwrap(),
            serde_json::json!("1234arkenstone")
        );
    }

    /// Ensures request payloads embedding an `OrbId` send `orbId` as a JSON
    /// string (e.g. `{"orbId":"1234"}`), matching the backend API contract,
    /// instead of a bare number.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_orb_id_in_request_payload() {
        #[derive(serde::Serialize)]
        struct Request {
            #[serde(rename = "orbId")]
            orb_id: OrbId,
        }

        let req = Request {
            orb_id: "1234".parse().unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"orbId":"1234arkenstone"}"#
        );
    }
}
