#[cfg(feature = "async")]
use crate::from_file;
use crate::from_file_blocking;
#[cfg(feature = "async")]
use futures::TryFutureExt;
#[cfg(feature = "async")]
use std::future;

/// An Arkenstone ID, formatted and serialized as `P` followed by eight uppercase hex digits.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct OrbId(u32);

impl std::fmt::Debug for OrbId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::fmt::Display for OrbId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "P{:08X}", self.0)
    }
}

impl std::str::FromStr for OrbId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Sysfs serial numbers and existing ORB_ID overrides are decimal.
        match s.strip_prefix('P') {
            Some(hex) => u32::from_str_radix(hex, 16).map(Self),
            None => s.parse().map(Self),
        }
    }
}

// Serialize/deserialize as a string, matching orb_id_linux's `OrbId` and the
// backend API, which expects `orbId` to be a string on every platform.
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
        for (serial, expected) in [
            (0, "P00000000"),
            (1234, "P000004D2"),
            (1428495495, "P55251C87"),
            (u32::MAX, "PFFFFFFFF"),
        ] {
            let id: OrbId = serial.to_string().parse().unwrap();
            assert_eq!(id.0, serial);
            assert_eq!(id.to_string(), expected);
            assert_eq!(format!("{id:?}"), expected);
            assert_eq!(format!("{id:#?}"), expected);
            assert_eq!(expected.parse::<OrbId>().unwrap(), id);
        }
        assert_eq!("P55251c87".parse::<OrbId>().unwrap().0, 1428495495);
    }

    #[test]
    fn test_invalid_orb_id() {
        for input in [
            "",
            "P",
            "Pxyz",
            "P100000000",
            "4294967296",
            "-1",
            "Q55251C87",
        ] {
            assert!(input.parse::<OrbId>().is_err(), "accepted {input:?}");
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_sync_get_orb_id_from_env() {
        for input in ["1428495495", "P55251C87"] {
            std::env::set_var("ORB_ID", input);
            let orb_id = OrbId::read_blocking().unwrap();
            assert_eq!(orb_id.to_string(), "P55251C87");
        }

        std::env::remove_var("ORB_ID");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial_test::serial]
    async fn test_async_get_orb_id_from_env() {
        for input in ["1428495495", "P55251C87"] {
            std::env::set_var("ORB_ID", input);
            let orb_id = OrbId::read().await.unwrap();
            assert_eq!(orb_id.to_string(), "P55251C87");
        }

        std::env::remove_var("ORB_ID");
    }

    #[test]
    #[serial_test::serial]
    fn test_sync_get_orb_id_from_file() {
        std::env::remove_var("ORB_ID");
        std::fs::write(SOC_SERIAL_NUMBER_PATH, "5678\n").unwrap();

        let orb_id = OrbId::read_blocking().unwrap();
        assert_eq!(orb_id.0, 5678);
        assert_eq!(orb_id.to_string(), "P0000162E");
        #[cfg(feature = "serde")]
        assert_eq!(serde_json::to_value(&orb_id).unwrap(), "P0000162E");

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
        assert_eq!(orb_id.to_string(), "P0000162E");
        #[cfg(feature = "serde")]
        assert_eq!(serde_json::to_value(&orb_id).unwrap(), "P0000162E");

        tokio::fs::remove_file(SOC_SERIAL_NUMBER_PATH)
            .await
            .unwrap();
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_orb_id() {
        for input in ["1428495495", "P55251C87"] {
            let id: OrbId = serde_json::from_value(serde_json::json!(input)).unwrap();
            assert_eq!(id.0, 1428495495);
            assert_eq!(
                serde_json::to_value(id).unwrap(),
                serde_json::json!("P55251C87")
            );
        }
    }

    /// Ensures request payloads embedding an `OrbId` send `orbId` as a JSON
    /// string (e.g. `{"orbId":"P55251C87"}`), matching the backend API contract,
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
            orb_id: "1428495495".parse().unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"orbId":"P55251C87"}"#
        );
    }
}
