//! The tags api
use eyre::{eyre, Context};
use iroh::NodeId;
use iroh_gossip::proto::TopicId;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{fmt::Display, str::FromStr};

use crate::HASH_CTX;

#[derive(Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Tag {
    pub domain: Domain,
    pub name: String,
}

impl Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.domain, self.name)
    }
}

/// Error when parsing a [Tag] from a string.
#[derive(Debug, thiserror::Error)]
#[error("failed to parse tag")]
pub struct ParseTagErr(#[from] eyre::Report);

impl FromStr for Tag {
    type Err = ParseTagErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((domain, name)) = s.split_once("/") else {
            return Err(eyre!("missing a `/`").into());
        };
        let domain: Domain = domain.parse().wrap_err("failed to parse domain")?;
        if name.is_empty() {
            return Err(eyre!("empty name string").into());
        }

        Ok(Tag {
            domain,
            name: name.to_owned(),
        })
    }
}

/// A domain name
#[derive(Debug, Eq, PartialEq, Hash, Ord, PartialOrd, derive_more::Display)]
pub struct Domain(String);

#[cfg(test)]
mod test_domain {
    use super::*;

    #[test]
    fn test_round_trip() {
        let upper = Domain::from_str("example.com").unwrap();
        let lower = Domain::from_str("eXaMple.com").unwrap();
        assert_eq!(upper, lower);
        assert_eq!(upper, "example.com");
    }

    #[test]
    fn test_invalid() {
        assert!(Domain::from_str("example").is_err(), "no tld");
        assert!(Domain::from_str("👉👈.com").is_err(), "must be ascii");
        assert!(Domain::from_str("").is_err(), "too short/empty");
        assert!(Domain::from_str("a.b").is_err(), "too short");
    }
}

/// Error when parsing a [Domain] from a string.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ParseDomainErr(#[from] eyre::Report);

impl FromStr for Domain {
    type Err = ParseDomainErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // TODO: use a real crate for domain name sanitization, this is just a placeholder
        if s.len() <= 3 {
            return Err(eyre!("not long enough").into());
        }
        if !s.contains('.') {
            return Err(eyre!("doesn't contain a `.`").into());
        }
        if !s.is_ascii() {
            return Err(eyre!("not ascii").into());
        }
        Ok(Domain(s.to_ascii_lowercase()))
    }
}

impl<T: AsRef<str>> PartialEq<T> for Domain {
    fn eq(&self, other: &T) -> bool {
        self.0 == other.as_ref()
    }
}

/// Topic for a blob's tag
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct TagTopic {
    pub tag: Tag,
}

impl TagTopic {
    pub(crate) fn to_id(&self) -> TopicId {
        let mut hasher: Sha256 = sha2::Digest::new();
        hasher.update(HASH_CTX);
        hasher.update("tag");

        hasher.update(self.tag.domain.0.as_bytes());
        hasher.update(self.tag.name.as_bytes());
        let hash: [u8; 32] = hasher.finalize().into();

        TopicId::from_bytes(hash)
    }
}

#[cfg(test)]
mod test_tag {
    use super::*;

    #[test]
    fn test_tag_round_trip() {
        let domain = Domain::from_str("example.com").unwrap();

        let s = "example.com/foo";
        let parsed = Tag::from_str(s).expect("failed to parse");
        assert_eq!(
            parsed,
            Tag {
                domain,
                name: "foo".to_owned()
            },
        );
        assert_eq!(format!("{parsed}"), s);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TagGossipMsg {
    hash: [u8; 32],
    node_id: NodeId,
    cert: Vec<u8>,
    cas: u64,
    sig: Vec<u8>,
}
