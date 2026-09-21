//! Encoding of the signed `hashes.json` manifest.

use std::collections::BTreeMap;

use data_encoding::HEXLOWER;

/// The wire format to encode. Negotiation and device-key policy belong to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ManifestFormat {
    V2_7,
    V2_8,
    /// Tier digests must cover the final user-encrypted bytes, not plaintext archives.
    V3_0 {
        tier_1: [u8; 32],
        tier_2: [u8; 32],
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("duplicate manifest entry")]
    DuplicateEntry,
    #[error("reserved manifest entry")]
    ReservedEntry,
    #[error("could not serialize hash manifest")]
    Serialization(#[from] serde_json::Error),
}

/// Exact manifest and signer output bytes, ready for archive assembly.
pub(crate) struct SignedManifest {
    pub hashes_json: Vec<u8>,
    pub hashes_signature: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum SigningError<E> {
    #[error("could not encode the manifest for signing")]
    Manifest(#[from] ManifestError),
    #[error("manifest signer failed")]
    Signer(#[source] E),
}

/// Encodes the manifest and calls `sign_digest` once with its raw SHA-256 digest.
///
/// The callback must sign this precomputed 32-byte digest without hashing it
/// again. Its returned bytes become `hashes.sign` unchanged. This function does
/// not inspect their signature format or verify the signer/identity. The caller
/// owns key access, retry policy and deadlines; no retries occur here.
/// Encoding failures do not invoke the signer. Signer failures return no signed
/// result and retain the caller's error as [`SigningError::Signer`].
pub(crate) fn encode_and_sign<'a, E>(
    version: ManifestFormat,
    hashes: impl IntoIterator<Item = (&'a str, [u8; 32])>,
    sign_digest: impl FnOnce(&[u8; 32]) -> Result<Vec<u8>, E>,
) -> Result<SignedManifest, SigningError<E>> {
    let hashes_json = encode(version, hashes)?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &hashes_json);
    let digest = digest
        .as_ref()
        .try_into()
        .expect("SHA-256 produces 32 bytes");
    let hashes_signature = sign_digest(digest).map_err(SigningError::Signer)?;
    Ok(SignedManifest {
        hashes_json,
        hashes_signature,
    })
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
pub(crate) fn encode<'a>(
    version: ManifestFormat,
    hashes: impl IntoIterator<Item = (&'a str, [u8; 32])>,
) -> Result<Vec<u8>, ManifestError> {
    let mut fields = BTreeMap::new();
    for (name, digest) in hashes {
        if matches!(
            name,
            "version" | "tier_1" | "tier_2" | "tier_3" | "tier_4" | "tier_5"
        ) {
            return Err(ManifestError::ReservedEntry);
        }
        if fields.insert(name, HEXLOWER.encode(&digest)).is_some() {
            return Err(ManifestError::DuplicateEntry);
        }
    }
    let wire_version = match version {
        ManifestFormat::V2_7 => "2.7",
        ManifestFormat::V2_8 => "2.8",
        ManifestFormat::V3_0 { tier_1, tier_2 } => {
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

#[cfg(test)]
#[path = "../tests/unit/manifest.rs"]
mod tests;
