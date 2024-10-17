use std::{borrow::Cow, fmt::Display, str::FromStr};

#[derive(serde::Serialize, Copy, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum Slot {
    A = 0,
    B = 1,
}

impl Slot {
    /// Returns the slot opposite to the current one.
    ///
    /// # Examples
    ///
    /// ```
    /// use orb_update_agent_core::Slot;
    ///
    /// assert_eq!(Slot::B, Slot::opposite(Slot::A));
    /// assert_eq!(Slot::A, Slot::opposite(Slot::B));
    /// ```
    pub fn opposite(self) -> Self {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}

impl Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Slot::A => "a",
            Slot::B => "b",
        };
        f.write_str(s)
    }
}

impl From<slot_ctrl::Slot> for Slot {
    fn from(slot: slot_ctrl::Slot) -> Self {
        match slot {
            slot_ctrl::Slot::A => Slot::A,
            slot_ctrl::Slot::B => Slot::B,
        }
    }
}

impl From<Slot> for slot_ctrl::Slot {
    fn from(slot: Slot) -> Self {
        match slot {
            Slot::A => slot_ctrl::Slot::A,
            Slot::B => slot_ctrl::Slot::B,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Failed to parse unknown string `{unknown}` as Slot")]
pub struct SlotParseError {
    unknown: String,
}

impl SlotParseError {
    pub fn unknown<'a, T: Into<Cow<'a, str>>>(val: T) -> Self {
        Self {
            unknown: val.into().to_string(),
        }
    }
}

impl<T> From<T> for SlotParseError
where
    T: AsRef<str>,
{
    fn from(unknown: T) -> Self {
        Self {
            unknown: unknown.as_ref().to_string(),
        }
    }
}

impl FromStr for Slot {
    type Err = SlotParseError;

    fn from_str(unknown: &str) -> Result<Self, Self::Err> {
        let slot = match unknown {
            "a" => Slot::A,
            "b" => Slot::B,
            other => return Err(other.into()),
        };
        Ok(slot)
    }
}

mod serde_imp {
    use std::str::FromStr;

    use serde::{de, Deserialize, Deserializer};

    use super::Slot;

    impl<'de> Deserialize<'de> for Slot {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            FromStr::from_str(&s).map_err(de::Error::custom)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SlotParseError;
    use crate::Slot;

    #[test]
    fn parsing_a_gives_slot_a() -> Result<(), SlotParseError> {
        let parsed = "a".parse()?;
        assert_eq!(Slot::A, parsed);
        Ok(())
    }

    #[test]
    fn parsing_b_gives_slot_b() -> Result<(), SlotParseError> {
        let parsed = "b".parse()?;
        assert_eq!(Slot::B, parsed);
        Ok(())
    }

    #[test]
    fn parsing_c_gives_error() {
        let parsed = "c".parse();
        let expected = Err::<Slot, _>(SlotParseError::unknown("c"));
        assert_eq!(expected, parsed);
    }
}
