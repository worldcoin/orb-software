//! Encoding of the signed `hashes.json` manifest.

use std::collections::BTreeMap;

use data_encoding::HEXLOWER;

/// The PCP version stamped in every manifest. `pcp.v1` evolves additively, so
/// new optional fields and files do not change it.
pub(crate) const VERSION: &str = "2.8";

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
    hashes: impl IntoIterator<Item = (&'a str, [u8; 32])>,
    sign_digest: impl FnOnce(&[u8; 32]) -> Result<Vec<u8>, E>,
) -> Result<SignedManifest, SigningError<E>> {
    let hashes_json = encode(hashes)?;
    let hashes_signature =
        sign_digest(&sha256(&hashes_json)).map_err(SigningError::Signer)?;
    Ok(SignedManifest {
        hashes_json,
        hashes_signature,
    })
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .expect("SHA-256 produces 32 bytes")
}

/// Encodes named SHA-256 digests as compact, lexicographically sorted JSON.
///
/// Names can represent payloads or salted metadata. Their digests are supplied
/// by the caller; this function does not verify their meaning or completeness.
/// Duplicate names and `version`, which the encoder owns, are rejected.
///
/// Signing must hash these exact returned bytes with SHA-256 and pass that raw
/// 32-byte digest to a prehash signer. Do not reformat or reserialize the JSON.
///
pub(crate) fn encode<'a>(
    hashes: impl IntoIterator<Item = (&'a str, [u8; 32])>,
) -> Result<Vec<u8>, ManifestError> {
    let mut fields = BTreeMap::new();
    for (name, digest) in hashes {
        if name == "version" {
            return Err(ManifestError::ReservedEntry);
        }
        if fields.insert(name, HEXLOWER.encode(&digest)).is_some() {
            return Err(ManifestError::DuplicateEntry);
        }
    }
    fields.insert("version", VERSION.to_owned());
    Ok(serde_json::to_vec(&fields)?)
}

#[cfg(test)]
#[path = "../tests/unit/manifest.rs"]
mod tests;
