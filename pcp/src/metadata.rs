//! `info.json` encoding and salted hashes for the current PCP wire format.
//!
//! The caller's `Info` is written as given, except for the fields the builder
//! owns: every `*_salt`, the multiframe image ID lists and, under redaction, all
//! image IDs. Absent fields are omitted. Image identifiers and the certificate
//! are not covered by salted hashes, and `info.json` is not hashed as a whole.
//! Inputs must come from an authorized source; this encoder does not
//! authenticate them. Returned buffers and temporary metadata are not
//! automatically zeroized.

use std::collections::BTreeMap;

use data_encoding::HEXLOWER;
use orb_pcp_defs::v1::Info;
use rand::{CryptoRng, RngCore};
use ring::digest::{Context, SHA256};

use crate::payload::sorted_json;

/// The higher-level builder must derive this choice from its single package
/// redaction decision, together with payload and manifest inclusion.
pub(crate) enum ImageIdPolicy<'a> {
    /// Image IDs become empty strings and lists.
    Redacted,
    /// Multiframe IDs in archive order, replacing the caller's lists.
    Included {
        left_multiframe: Vec<&'a str>,
        right_multiframe: Vec<&'a str>,
    },
}

pub(crate) struct EncodedMetadata {
    pub info_json: Vec<u8>,
    /// Salted identity metadata only; no image IDs or certificate digest.
    pub hashes: BTreeMap<&'static str, [u8; 32]>,
}

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("could not generate metadata salt")]
    Randomness(#[source] rand::Error),
    #[error("could not serialize metadata")]
    Serialization(#[from] serde_json::Error),
}

/// Encodes sorted, compact JSON and hashes each present salted value followed by
/// its lowercase-hex salt (not the raw salt bytes). Each salt uses 16 fresh
/// random bytes and replaces any salt the caller supplied; an absent value gets
/// no salt and no hash. The caller supplies a cryptographically secure RNG;
/// failures return no encoded result. No retries occur here.
pub(crate) fn encode(
    info: &Info,
    images: ImageIdPolicy<'_>,
    rng: &mut (impl RngCore + CryptoRng),
) -> Result<EncodedMetadata, MetadataError> {
    let mut info = info.clone();
    match images {
        ImageIdPolicy::Redacted => {
            for id in [
                &mut info.left_ir_image_id,
                &mut info.right_ir_image_id,
                &mut info.thumbnail_image_id,
            ] {
                *id = Some(String::new());
            }
            for ids in [
                &mut info.left_ir_multiframe_image_ids,
                &mut info.right_ir_multiframe_image_ids,
                &mut info.left_iris_code_aggregate_image_ids,
                &mut info.right_iris_code_aggregate_image_ids,
            ] {
                ids.clear();
            }
        }
        ImageIdPolicy::Included {
            left_multiframe,
            right_multiframe,
        } => {
            let owned = |ids: Vec<&str>| ids.into_iter().map(str::to_owned).collect();
            info.left_ir_multiframe_image_ids = owned(left_multiframe);
            info.right_ir_multiframe_image_ids = owned(right_multiframe);
        }
    }
    let mut hashes = BTreeMap::new();
    // Salt generation order is part of deterministic compatibility with seeded callers.
    for (name, value, salt) in [
        ("signup_id", &info.signup_id, &mut info.signup_id_salt),
        (
            "signup_reason",
            &info.signup_reason,
            &mut info.signup_reason_salt,
        ),
        ("orb_id", &info.orb_id, &mut info.orb_id_salt),
        ("operator_id", &info.operator_id, &mut info.operator_id_salt),
        ("timestamp", &info.timestamp, &mut info.timestamp_salt),
        ("qr_code", &info.qr_code, &mut info.qr_code_salt),
        (
            "id_commitment",
            &info.id_commitment,
            &mut info.id_commitment_salt,
        ),
        (
            "software_version",
            &info.software_version,
            &mut info.software_version_salt,
        ),
        ("orb_country", &info.orb_country, &mut info.orb_country_salt),
        (
            "device_public_key",
            &info.device_public_key,
            &mut info.device_public_key_salt,
        ),
    ] {
        *salt = None;
        let Some(value) = value else {
            continue;
        };
        let mut bytes = [0; 16];
        rng.try_fill_bytes(&mut bytes)
            .map_err(MetadataError::Randomness)?;
        let fresh = HEXLOWER.encode(&bytes);
        let mut hash = Context::new(&SHA256);
        hash.update(value.as_bytes());
        hash.update(fresh.as_bytes());
        hashes.insert(
            name,
            hash.finish()
                .as_ref()
                .try_into()
                .expect("SHA-256 produces 32 bytes"),
        );
        *salt = Some(fresh);
    }
    Ok(EncodedMetadata {
        info_json: sorted_json(serde_json::to_value(&info)?)?,
        hashes,
    })
}

#[cfg(test)]
#[path = "../tests/unit/metadata.rs"]
mod tests;
