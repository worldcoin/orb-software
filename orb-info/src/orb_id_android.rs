#[cfg(feature = "async")]
use crate::from_file;
use crate::from_file_blocking;

/// An Arkenstone ID, displayed and serialized as `P` followed by eight uppercase hex digits.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct OrbId(u32);

impl std::fmt::Debug for OrbId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Arkenstone id: {self} - {}", self.0)
    }
}

impl std::fmt::Display for OrbId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "P{:08X}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("expected 'P' followed by exactly eight hexadecimal digits")]
pub struct ParseErr;

impl std::str::FromStr for OrbId {
    type Err = ParseErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix('P').ok_or(ParseErr)?;
        if hex.len() != 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ParseErr);
        }
        u32::from_str_radix(hex, 16).map(Self).map_err(|_| ParseErr)
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
    ParseSerial(#[from] std::num::ParseIntError),
    #[error(transparent)]
    ParseOrbId(#[from] ParseErr),
}

#[cfg(not(test))]
const SOC_SERIAL_NUMBER_PATH: &str = "/sys/devices/soc0/serial_number";
#[cfg(test)]
const SOC_SERIAL_NUMBER_PATH: &str = "./test_soc_serial_number";

impl OrbId {
    fn from_soc_serial_number(s: &str) -> Result<Self, std::num::ParseIntError> {
        s.parse().map(Self)
    }

    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, ReadErr> {
        if let Ok(s) = std::env::var("ORB_ID") {
            return Ok(s.parse()?);
        }
        let s = from_file(SOC_SERIAL_NUMBER_PATH).await?;
        Ok(Self::from_soc_serial_number(&s)?)
    }

    pub fn read_blocking() -> Result<Self, ReadErr> {
        if let Ok(s) = std::env::var("ORB_ID") {
            return Ok(s.parse()?);
        }
        let s = from_file_blocking(SOC_SERIAL_NUMBER_PATH)?;
        Ok(Self::from_soc_serial_number(&s)?)
    }
}

#[cfg(any(test, feature = "testing"))]
pub fn test_orb_id() -> OrbId {
    OrbId(666666)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_soc_serial_number() {
        for (serial, expected) in [
            (0, "P00000000"),
            (1234, "P000004D2"),
            (876120561, "P343889F1"),
            (1428495495, "P55251C87"),
            (u32::MAX, "PFFFFFFFF"),
        ] {
            let id = OrbId::from_soc_serial_number(&serial.to_string()).unwrap();
            assert_eq!(id.0, serial);
            assert_eq!(id.to_string(), expected);
            assert_eq!(expected.parse::<OrbId>().unwrap(), id);
        }
        for input in ["", "P55251C87", "55251C87", "-1", "4294967296"] {
            assert!(OrbId::from_soc_serial_number(input).is_err());
        }
    }

    #[test]
    fn test_from_str() {
        for (input, serial, expected) in [
            ("P00000000", 0, "P00000000"),
            ("P00001234", 0x1234, "P00001234"),
            ("P55251C87", 1428495495, "P55251C87"),
            ("P55251c87", 1428495495, "P55251C87"),
            ("Pffffffff", u32::MAX, "PFFFFFFFF"),
        ] {
            let id: OrbId = input.parse().unwrap();
            assert_eq!(id.0, serial);
            assert_eq!(id.to_string(), expected);
        }
    }

    #[test]
    fn test_debug() {
        let id: OrbId = "P55251C87".parse().unwrap();
        assert_eq!(format!("{id:?}"), "Arkenstone id: P55251C87 - 1428495495");
        assert_eq!(format!("{id:#?}"), "Arkenstone id: P55251C87 - 1428495495");
        assert!(format!("{id:?}").parse::<OrbId>().is_err());
    }

    #[test]
    fn test_invalid_orb_id() {
        for input in [
            "",
            "P",
            "Pxyz",
            "P1234",
            "P+1234",
            "P+0001234",
            "P-0001234",
            "P0000000G",
            "P100000000",
            "1428495495",
            "4294967296",
            "-1",
            "Q55251C87",
            "p55251C87",
            " P55251C87",
            "P55251C87\n",
            "P５５２５１Ｃ８７",
        ] {
            assert!(input.parse::<OrbId>().is_err(), "accepted {input:?}");
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_sync_get_orb_id_from_env() {
        for input in ["P55251c87", "P55251C87"] {
            std::env::set_var("ORB_ID", input);
            let orb_id = OrbId::read_blocking().unwrap();
            assert_eq!(orb_id.to_string(), "P55251C87");
        }
        std::env::set_var("ORB_ID", "1428495495");
        assert!(matches!(
            OrbId::read_blocking(),
            Err(ReadErr::ParseOrbId(_))
        ));

        std::env::remove_var("ORB_ID");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial_test::serial]
    async fn test_async_get_orb_id_from_env() {
        for input in ["P55251c87", "P55251C87"] {
            std::env::set_var("ORB_ID", input);
            let orb_id = OrbId::read().await.unwrap();
            assert_eq!(orb_id.to_string(), "P55251C87");
        }
        std::env::set_var("ORB_ID", "1428495495");
        assert!(matches!(OrbId::read().await, Err(ReadErr::ParseOrbId(_))));

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
        for input in ["P55251c87", "P55251C87"] {
            let id: OrbId = serde_json::from_value(serde_json::json!(input)).unwrap();
            assert_eq!(id.0, 1428495495);
            assert_eq!(
                serde_json::to_value(id).unwrap(),
                serde_json::json!("P55251C87")
            );
        }
        for input in ["1428495495", "P+1234", "Ark55251c87"] {
            assert!(serde_json::from_value::<OrbId>(serde_json::json!(input)).is_err());
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
            orb_id: "P55251C87".parse().unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"orbId":"P55251C87"}"#
        );
    }
}
