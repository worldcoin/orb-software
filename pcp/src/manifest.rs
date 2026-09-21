//! Encoding of the signed `hashes.json` manifest.

use std::collections::BTreeMap;

use data_encoding::HEXLOWER;

/// The wire format to encode. Negotiation and device-key policy belong to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    V2_7,
    V2_8,
    /// Tier digests must cover the final user-encrypted bytes, not plaintext archives.
    V3_0 {
        tier_1: [u8; 32],
        tier_2: [u8; 32],
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("duplicate manifest entry")]
    DuplicateEntry,
    #[error("reserved manifest entry")]
    ReservedEntry,
    #[error("could not serialize hash manifest")]
    Serialization(#[from] serde_json::Error),
}

/// Encodes named SHA-256 digests as compact, lexicographically sorted JSON.
///
/// Names can represent payloads or salted metadata. Their digests are supplied
/// by the caller; this function does not verify their meaning or completeness.
/// Duplicate names and `version`, `tier_1` through `tier_5` are rejected in every
/// format. These reserved fields are owned by the encoder.
///
/// Signing must hash these exact returned bytes with SHA-256 and pass that raw
/// 32-byte digest to a prehash signer. Do not reformat or reserialize the JSON.
///
/// ```
/// use orb_pcp::manifest::{encode, Version};
///
/// // SHA-256 digests of the exact payload bytes, computed by the caller.
/// let payload_hashes = [("example.bin", [0xab; 32])];
/// let hashes_json = encode(Version::V2_8, payload_hashes)?;
/// # Ok::<(), orb_pcp::manifest::Error>(())
/// ```
pub fn encode<'a>(
    version: Version,
    hashes: impl IntoIterator<Item = (&'a str, [u8; 32])>,
) -> Result<Vec<u8>, Error> {
    let mut fields = BTreeMap::new();
    for (name, digest) in hashes {
        if matches!(
            name,
            "version" | "tier_1" | "tier_2" | "tier_3" | "tier_4" | "tier_5"
        ) {
            return Err(Error::ReservedEntry);
        }
        if fields.insert(name, HEXLOWER.encode(&digest)).is_some() {
            return Err(Error::DuplicateEntry);
        }
    }
    let wire_version = match version {
        Version::V2_7 => "2.7",
        Version::V2_8 => "2.8",
        Version::V3_0 { tier_1, tier_2 } => {
            fields.insert("tier_1", HEXLOWER.encode(&tier_1));
            fields.insert("tier_2", HEXLOWER.encode(&tier_2));
            for name in ["tier_3", "tier_4", "tier_5"] {
                fields.insert(name, HEXLOWER.encode(&[0; 32]));
            }
            "3.0"
        }
    };
    fields.insert("version", wire_version.to_owned());
    Ok(serde_json::to_vec(&fields)?)
}
