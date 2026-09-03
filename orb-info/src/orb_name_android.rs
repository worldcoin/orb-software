use crate::orb_id::{self, OrbId};
use bip39;
use std::fmt::Display;
use std::str::FromStr;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OrbName(OrbId);

pub(crate) const UNKNOWN: OrbName = OrbName(orb_id::UNKNOWN);

pub type ReadErr = crate::orb_id::ReadErr;

impl OrbName {
    #[cfg(feature = "async")]
    pub async fn read() -> Result<Self, ReadErr> {
        let id = OrbId::read().await?;
        Ok(Self(id))
    }

    pub fn read_blocking() -> Result<Self, ReadErr> {
        let id = OrbId::read_blocking()?;
        Ok(Self(id))
    }

    /// return the orb-name, if fail return UNKNOWN
    #[cfg(feature = "async")]
    pub async fn read_unfallable() -> Self {
        Self::read().await.unwrap_or(UNKNOWN)
    }

    /// return the orb-name, if fail return UNKNOWN
    pub fn read_blocking_unfallable() -> Self {
        Self::read_blocking().unwrap_or(UNKNOWN)
    }
}

/// Splits the orb id into three BIP39 word indices (11 bits each). The
/// 32-bit id is padded with one parity (CRC-1) bit to fill the 33 bits
/// needed for three full-range 11-bit words.
fn id_to_words(value: u32) -> String {
    // TODO It could be any language, maybe depending on the orb's locale
    let words = bip39::Language::English.word_list();
    let crc_bit = value.count_ones() % 2;
    let combined = ((value as u64) << 1) | crc_bit as u64;
    let w1 = (combined & 0x7FF) as usize;
    let w2 = ((combined >> 11) & 0x7FF) as usize;
    let w3 = ((combined >> 22) & 0x7FF) as usize;
    format!("{}-{}-{}", words[w1], words[w2], words[w3])
}

#[derive(Debug, thiserror::Error)]
pub enum ParseOrbNameError {
    #[error("expected 3 words separated by '-', got {0}")]
    WordCount(usize),
    #[error("unknown word: {0}")]
    UnknownWord(String),
    #[error("checksum mismatch")]
    ChecksumMismatch,
}

impl FromStr for OrbName {
    type Err = ParseOrbNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let words: Vec<&str> = s.split('-').collect();
        let [w1, w2, w3] = words[..] else {
            return Err(ParseOrbNameError::WordCount(words.len()));
        };

        let index = |word: &str| {
            bip39::Language::English
                .find_word(word)
                .ok_or_else(|| ParseOrbNameError::UnknownWord(word.to_string()))
        };
        let combined =
            index(w1)? as u64 | (index(w2)? as u64) << 11 | (index(w3)? as u64) << 22;

        let crc_bit = (combined & 1) as u32;
        let value = (combined >> 1) as u32;
        if value.count_ones() % 2 != crc_bit {
            return Err(ParseOrbNameError::ChecksumMismatch);
        }

        Ok(Self(OrbId(value)))
    }
}

impl Display for OrbName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&id_to_words(self.0 .0))
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for OrbName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for OrbName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
