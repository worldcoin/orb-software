use crate::s3_region::S3Region;
use eyre::{eyre, Error, Result};
use rand::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, path::Path, str::FromStr};
use uuid::Uuid;

const VERSION: u8 = 0;

#[derive(Serialize, Deserialize, JsonSchema, Clone, Default, Eq, PartialEq, Debug)]
struct WldDataId {
    /// The version of this structure.
    version: u8,
    /// The AWS region this object will be uploaded to.
    s3_region: S3Region,
    /// A globally unique id for the signup.
    signup_id: [u8; 10],
    /// An id for some data (e.g. an image), unique within the signup.
    data_id: u32,
}

impl WldDataId {
    fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[0] = self.version;
        bytes[1] = self.s3_region as u8;
        bytes[2..12].copy_from_slice(&self.signup_id);
        bytes[12..16].copy_from_slice(&self.data_id.to_le_bytes());
        bytes
    }

    fn from_bytes(bytes: [u8; 16]) -> Self {
        let mut signup_id = [0; 10];
        signup_id.copy_from_slice(&bytes[2..12]);
        Self {
            version: bytes[0],
            s3_region: S3Region::from_byte(bytes[1]),
            signup_id,
            data_id: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        }
    }

    fn to_uuid(&self) -> Uuid {
        Uuid::from_bytes(self.to_bytes())
    }
}

impl From<Uuid> for WldDataId {
    fn from(uuid: Uuid) -> WldDataId {
        Self::from_bytes(*uuid.as_bytes())
    }
}

impl FromStr for WldDataId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Uuid::parse_str(s)?.into())
    }
}

impl fmt::Display for WldDataId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.to_uuid()
                .simple()
                .encode_lower(&mut Uuid::encode_buffer())
        )
    }
}

#[allow(missing_docs)]
#[derive(Serialize, Deserialize, JsonSchema, Clone, Default, Debug, Eq, PartialEq)]
pub struct SignupId(WldDataId);

impl SignupId {
    /// Generates a globally unique signup id given the S3 region.
    #[must_use]
    pub fn new(s3_region: S3Region) -> Self {
        Self(WldDataId {
            version: VERSION,
            s3_region,
            signup_id: thread_rng().r#gen(),
            data_id: 0,
        })
    }

    /// Parses a signup id from the signup directory.
    pub fn from_signup_dir(path: &Path) -> Result<Self> {
        path.file_name()
            .ok_or_else(|| eyre!("Invalid path {:?}", path))?
            .to_string_lossy()
            .parse()
    }

    /// Converts the signup ID to a 6-digit hex PIN string.
    /// Uses SHA-256 hash of the signup_id bytes to minimize collision probability.
    #[must_use]
    pub fn to_pin_string(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.0.signup_id);
        let result = hasher.finalize();
        format!(
            "{:06x}",
            u32::from_be_bytes([result[0], result[1], result[2], 0]) >> 8
        )
    }
}

impl fmt::Display for SignupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for SignupId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl From<ImageId> for SignupId {
    fn from(image_id: ImageId) -> Self {
        let mut tmp_id = image_id.0;
        tmp_id.data_id = 0;
        Self(tmp_id)
    }
}

#[allow(missing_docs)]
#[derive(Serialize, Deserialize, JsonSchema, Clone, Default, Debug, Eq, PartialEq)]
pub struct ImageId(WldDataId);

impl ImageId {
    /// Generates a new image id, given a signup id and the image timestamp.
    #[must_use]
    pub fn new(signup_id: &SignupId, hash: u32) -> Self {
        let mut new_id = signup_id.0.clone();
        new_id.data_id = hash;
        Self(new_id)
    }

    /// Parses an image id from an image path.
    pub fn from_image_path(path: &Path) -> Result<Self> {
        path.file_stem()
            .ok_or_else(|| eyre!("Invalid path {:?}", path))?
            .to_string_lossy()
            .parse()
    }
}

impl fmt::Display for ImageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ImageId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ImageId(s.parse()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::Result;

    #[test]
    fn test_object_id() -> Result<()> {
        let signup_id = SignupId::new(S3Region::Unknown);
        let image_id = ImageId::new(&signup_id, 123_123);
        let s = image_id.to_string();
        assert_eq!(s.parse::<WldDataId>()?.to_string(), s);
        Ok(())
    }

    #[test]
    fn test_sensitivity() {
        let signup_id = SignupId::new(S3Region::Unknown);
        assert_ne!(
            ImageId::new(&signup_id, 0).to_string(),
            ImageId::new(&signup_id, 1).to_string()
        );
    }

    #[test]
    fn test_pin_string() {
        let signup_id = SignupId::new(S3Region::Unknown);
        let pin = signup_id.to_pin_string();

        // Verify it's exactly 6 hex characters
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_hexdigit()));

        // Verify deterministic: same signup_id produces same PIN
        assert_eq!(pin, signup_id.to_pin_string());

        // Verify different signup_ids produce different PINs (with high probability)
        let other_signup_id = SignupId::new(S3Region::Unknown);
        assert_ne!(pin, other_signup_id.to_pin_string());
    }
}
