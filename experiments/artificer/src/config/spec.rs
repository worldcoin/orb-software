//! Schema for the spec file (aka, artificer.toml)

use std::{collections::HashMap, path::PathBuf, str::FromStr};

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};

use super::{sources::Source, ArtifactName};

/// The spec file, aka `artificer.toml`. Describes the full set of artifacts
/// to download.
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    pub artificer: Artificer,
    pub artifacts: HashMap<ArtifactName, Artifact>,
    pub extractors: HashMap<ExtractorName, CustomExtractor>,
}

/// `[artificer]` toplevel table
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Artificer {
    pub version: Version,
    pub out_dir: PathBuf,
}

/// Info for a particular artifact. See also, [`ArtifactName`].
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Artifact {
    #[serde(flatten)]
    pub source: Source,
    pub hash: Option<Hash>,
    pub extractor: Option<ExtractorName>,
}

/// `[extractors.<extractor-name>]`. See [`CustomExtractor`] for custom extractors.
/// Note that there also will be built-in extractors in the future.
#[derive(Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Clone)]
pub struct ExtractorName(pub String);

/// Custom extractor, referenced by [`ExtractorName`].
#[derive(Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CustomExtractor {
    pub run: String,
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum Hash {
    Hash(cacache::Integrity),
    /// Dummy hash value that will never correct. Specified by an empty string.
    Dummy,
    False,
}

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Hash(h) => h.serialize(serializer),
            Self::Dummy => serializer.serialize_str(""),
            Self::False => serializer.serialize_bool(false),
        }
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HashVisitor;

        impl serde::de::Visitor<'_> for HashVisitor {
            type Value = Hash;

            fn expecting(
                &self,
                formatter: &mut std::fmt::Formatter,
            ) -> std::fmt::Result {
                write!(
                    formatter,
                    "either `false`, empty string, or a subresource integrity \
                    string (aka hash)"
                )
            }

            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> Result<Self::Value, E> {
                if v {
                    Err(E::invalid_value(serde::de::Unexpected::Bool(true), &self))
                } else {
                    Ok(Hash::False)
                }
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v.is_empty() {
                    Ok(Hash::Dummy)
                } else {
                    let integrity =
                        cacache::Integrity::from_str(v).map_err(E::custom)?;
                    Ok(Hash::Hash(integrity))
                }
            }
        }

        deserializer.deserialize_any(HashVisitor)
    }
}

#[cfg(test)]
mod test {
    use color_eyre::{eyre::WrapErr, Result};

    use super::Spec;

    fn deserialize_example_spec() -> Result<Spec> {
        let file_contents = include_str!("example.toml");
        toml::from_str(file_contents).wrap_err("failed to deserialize example spec")
    }

    #[test]
    fn test_roundtrip_example() -> Result<()> {
        let deserialized = deserialize_example_spec()?;
        let serialized =
            toml::to_string(&deserialized).wrap_err("failed to serialize")?;
        let deserialized_again = toml::from_str(&serialized)
            .wrap_err("failed to deserialize from serialized")?;
        assert_eq!(deserialized, deserialized_again);

        Ok(())
    }
}
