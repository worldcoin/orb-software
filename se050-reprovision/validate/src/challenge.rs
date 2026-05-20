//! [`Nonce`] type

use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

const N_BYTES: usize = 16;

/// A random nonce challenge. Should be sent to the orb and kept to validate along
/// with the corresponding proof.
#[derive(
    Debug,
    Eq,
    PartialEq,
    Clone,
    Copy,
    Hash,
    Ord,
    PartialOrd,
    derive_more::AsRef,
    derive_more::From,
    derive_more::Into,
    Serialize,
    Deserialize,
)]
#[serde(transparent)]
pub struct Nonce([u8; N_BYTES]);

impl Nonce {
    pub const LEN: usize = N_BYTES;

    /// Generate a new random nonce
    pub fn random() -> Self {
        Self::from_rng(rand::rng())
    }

    /// Generate a new random nonce from a specific RNG. Useful for deterministic tests
    pub fn from_rng(mut rng: impl rand::CryptoRng) -> Self {
        let mut buf = [0; Self::LEN];
        rng.fill_bytes(&mut buf);

        Self(buf)
    }

    pub fn as_array(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl Display for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x")?;
        for &b in self.0.iter() {
            write!(f, "{:X}", b)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn test_nonce_display() {
        assert_eq!(
            "0x4550F29373865F63201A8685962D9078",
            format!("{}", Nonce::from(hex!("4550F29373865F63201A8685962D9078")))
        );
    }
}
