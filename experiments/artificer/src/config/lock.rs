//! Schema for artificer.lock and out.lock

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::sources::Source;
use super::spec::Artifact;
use super::ArtifactName;

/// The locks for a [`Spec`]. Stored in the lockfile, aka `artificer.lock`. Specifies
/// last observed hashes for every artifact.
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LockedSpec {
    /// The version of the overall lockfile syntax
    pub version: u8,
    pub artifacts: HashMap<ArtifactName, Artifact>,
}

/// An artifact in the lock file.
#[expect(dead_code)]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LockedArtifact {
    pub source: Source,
    pub hash: cacache::Integrity,
}

#[cfg(test)]
mod test {
    use color_eyre::{eyre::WrapErr, Result};

    use super::LockedSpec;

    fn deserialize_example_lockfile() -> Result<LockedSpec> {
        let file_contents = include_str!("example.lock");
        toml::from_str(file_contents).wrap_err("failed to deserialize example lockfile")
    }

    #[test]
    fn test_roundtrip_example() -> Result<()> {
        let deserialized = deserialize_example_lockfile()?;
        let serialized =
            toml::to_string(&deserialized).wrap_err("failed to serialize")?;
        let deserialized_again = toml::from_str(&serialized)
            .wrap_err("failed to deserialize from serialized")?;
        assert_eq!(deserialized, deserialized_again);

        Ok(())
    }
}
