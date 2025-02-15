pub use hex::FromHexError;

use std::{fmt::Display, hash::Hash, str::FromStr};
use thiserror::Error;

use crate::from_binary_blocking;

/// Boilerplate for OrbId*
macro_rules! impl_orb_id {
    (
        $(#[$($enum_attrs:tt)*])*
        $vis:vis struct $name:ident<$n:literal>;
    ) => {
        $(#[$($enum_attrs)*])*
        #[derive(Debug, Clone, Eq, PartialEq)]
        $vis struct $name {
            string: String,
            bytes: [u8; $n],
        }

        impl $name {
            pub const N_BYTES: usize = $n;

            pub fn new(bytes: [u8; $n]) -> Self {
                Self {
                    string: hex::encode(&bytes),
                    bytes,
                }
            }

            pub fn as_str(&self) -> &str {
                &self.string
            }

            pub fn as_bytes(&self) -> &[u8; $n] {
                &self.bytes
            }
        }

        impl FromStr for $name {
            type Err = FromHexError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let mut result = Self {
                    string: s.to_lowercase(),
                    bytes: [0; $n],
                };
                hex::decode_to_slice(s, &mut result.bytes)?;
                Ok(result)
            }
        }

        impl Hash for $name {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.bytes[..usize::min($n, 4)].hash(state)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(&self.string)
            }
        }
    };
}

impl_orb_id! {
    /// A short [`OrbId`]. These shorter orb ids are what all EV4+ orbs use.
    ///
    /// # Example
    /// ```
    /// # use orb_info::orb_id::OrbIdShort;
    /// let id: OrbIdShort = "ea2ea744".parse().unwrap();
    /// assert_eq!(id.as_str(), "ea2ea744");
    /// ```
    pub struct OrbIdShort<4>;
}

impl_orb_id! {
    /// A long [`OrbId`]. These longer orb ids are what pre-EV4 orbs used to use.
    ///
    /// # Example
    /// ```
    /// # use orb_info::orb_id::OrbIdLong;
    /// let s = "ea2ea744295c5dacb12a825713f9cec1a2f4d63d86803a15fe580d6a468ab6d2";
    /// let id: OrbIdLong = s.parse().unwrap();
    /// assert_eq!(id.as_str(), s);
    /// ```
    pub struct OrbIdLong<32>;
}

/// An orb id.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum OrbId {
    Short(OrbIdShort),
    Long(OrbIdLong),
}

#[derive(Debug, Error)]
pub enum ReadErr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl OrbId {
    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, ReadErr> {
        use crate::from_binary;

        let id = if let Ok(s) = std::env::var("ORB_ID") {
            Ok(s.trim().to_string())
        } else {
            from_binary("orb-id").await
        }?;
        Ok(Self::from_str(&id).unwrap())
    }

    pub fn read_blocking() -> Result<Self, ReadErr> {
        let id = if let Ok(s) = std::env::var("ORB_ID") {
            Ok(s.trim().to_string())
        } else {
            from_binary_blocking("orb-id")
        }?;
        Ok(Self::from_str(&id).unwrap())
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Short(id) => id.as_str(),
            Self::Long(id) => id.as_str(),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Short(id) => id.as_bytes(),
            Self::Long(id) => id.as_bytes(),
        }
    }
}

impl From<OrbIdShort> for OrbId {
    fn from(value: OrbIdShort) -> Self {
        Self::Short(value)
    }
}

impl From<OrbIdLong> for OrbId {
    fn from(value: OrbIdLong) -> Self {
        Self::Long(value)
    }
}

impl FromStr for OrbId {
    type Err = FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(id) = s.parse::<OrbIdShort>() {
            return Ok(Self::from(id));
        }
        s.parse::<OrbIdLong>().map(Self::from)
    }
}

impl Display for OrbId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Short(id) => id.fmt(f),
            Self::Long(id) => id.fmt(f),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_round_trip() {
        let short = "ea2ea744";
        let long = "ea2ea744295c5dacb12a825713f9cec1a2f4d63d86803a15fe580d6a468ab6d2";
        let caps = short.to_uppercase();
        let caps = &caps;
        let repeated = String::from("a").repeat(64);
        let repeated = &repeated;

        let ids = [short, long, caps, repeated];
        for id in ids {
            let lower = OrbId::from_str(id).expect("failed lower");
            let upper = OrbId::from_str(&id.to_uppercase()).expect("failed upper");

            assert_eq!(lower, upper);
            assert_eq!(lower.as_str(), upper.as_str());
            assert_eq!(lower.as_bytes(), upper.as_bytes());
        }
    }

    #[test]
    fn test_invalid_hex() {
        let ids = ["", "a", "abc", "gg", &String::from("a").repeat(65)];
        for id in ids {
            assert!(OrbId::from_str(id).is_err(), "failed on value `{id}`");
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_sync_get_orb_id_from_env() {
        let test_id = "abcd1234";
        std::env::set_var("ORB_ID", test_id);

        let orb_id = OrbId::read_blocking().unwrap();
        assert_eq!(orb_id.as_str(), test_id);

        // Test caching works
        let cached_result = orb_id.as_str();
        assert_eq!(cached_result, test_id);

        std::env::remove_var("ORB_ID");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial_test::serial]
    async fn test_async_get_orb_id_from_env() {
        let test_id = "abcd1234";
        std::env::set_var("ORB_ID", test_id);

        let orb_id = OrbId::read().await.unwrap();
        assert_eq!(orb_id.as_str(), test_id);

        // Test caching works
        let cached_result = orb_id.as_str();
        assert_eq!(cached_result, test_id);

        std::env::remove_var("ORB_ID");
    }

    #[test]
    #[serial_test::serial]
    fn test_sync_get_orb_id_binary_failure() {
        std::env::remove_var("ORB_ID");

        // TODO(@paulquinn00): Tests should not rely on state from host environment
        // This should error since orb-id binary likely doesn't exist in test environment
        let Err(ReadErr::Io(err)) = OrbId::read_blocking() else {
            panic!("expected io error");
        };
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound)
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    #[serial_test::serial]
    async fn test_async_get_orb_id_binary_failure() {
        std::env::remove_var("ORB_ID");

        // TODO(@paulquinn00): Tests should not rely on host environment
        // This should panic since orb-id binary likely doesn't exist in test environment
        let Err(ReadErr::Io(err)) = OrbId::read().await else {
            panic!("expected io error");
        };
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound)
    }
}
