//! Metadata encoding and salted hashes for the current PCP wire format.
//!
//! Image identifiers and the certificate are not covered by these hashes, and
//! `info.json` is not hashed as a whole. Inputs must come from an authorized
//! source; this encoder does not authenticate them. Returned buffers and
//! temporary metadata are not automatically zeroized.

use std::{collections::BTreeMap, time::SystemTime};

use data_encoding::{BASE64, HEXLOWER};
use rand::{CryptoRng, RngCore};
use ring::digest::{Context, SHA256};
use serde_json::json;

/// Caller-authorized metadata. The legacy manifest covers salted identity fields,
/// but not the certificate, image IDs or `info.json` as a whole. Construction
/// does not authenticate these inputs.
pub struct PackageInfo<'a> {
    pub signup_id: &'a str,
    pub signup_reason: &'a str,
    pub orb_id: &'a str,
    pub operator_id: &'a str,
    pub capture_start: SystemTime,
    pub qr_code: &'a str,
    pub id_commitment: &'a str,
    pub software_version: &'a str,
    pub orb_country: &'a str,
    /// Raw certificate bytes, encoded as padded standard Base64.
    pub orb_public_key_certificate: &'a [u8],
    /// Omitted together with its salt and hash when absent.
    pub device_public_key: Option<&'a str>,
}

pub(crate) struct IrisImageIds<'a> {
    pub primary: &'a str,
    pub multiframe: &'a [&'a str],
}

/// Availability is distinct from redaction. The current wire profile requires
/// both eye groups and a thumbnail ID; absent groups return an error. Optional
/// groups preserve that distinction for future partial-data support.
pub(crate) struct ImageIds<'a> {
    pub left: Option<IrisImageIds<'a>>,
    pub right: Option<IrisImageIds<'a>>,
    pub thumbnail: Option<&'a str>,
    pub left_iris_code_aggregate: &'a [&'a str],
    pub right_iris_code_aggregate: &'a [&'a str],
}

/// The higher-level builder must derive this choice from its single package
/// redaction decision, together with payload and manifest inclusion.
pub(crate) enum ImageIdPolicy<'a> {
    Redacted,
    Included(ImageIds<'a>),
}

pub(crate) struct EncodedMetadata {
    pub info_json: Vec<u8>,
    /// Salted identity metadata only; no image IDs or certificate digest.
    pub hashes: BTreeMap<&'static str, [u8; 32]>,
}

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("missing required metadata image ID: {field}")]
    MissingImageId { field: &'static str },
    #[error("could not generate metadata salt")]
    Randomness(#[source] rand::Error),
    #[error("could not serialize metadata")]
    Serialization(#[from] serde_json::Error),
}

/// Encodes sorted, compact JSON and hashes each identity value followed by its
/// lowercase-hex salt (not the raw salt bytes). Each salt uses 16 fresh random
/// bytes. The caller supplies a cryptographically secure RNG; failures return
/// no encoded result. No retries occur here.
///
/// Capture time uses whole Unix seconds, clamping pre-epoch times to zero.
/// Redaction clears all image IDs but retains identity metadata and its hashes.
/// Missing included image groups are rejected before drawing randomness.
pub(crate) fn encode(
    info: &PackageInfo<'_>,
    images: &ImageIdPolicy<'_>,
    rng: &mut (impl RngCore + CryptoRng),
) -> Result<EncodedMetadata, MetadataError> {
    let empty_eye = IrisImageIds {
        primary: "",
        multiframe: &[],
    };
    let (left, right, thumbnail, left_aggregate, right_aggregate) = match images {
        ImageIdPolicy::Redacted => (&empty_eye, &empty_eye, "", &[][..], &[][..]),
        ImageIdPolicy::Included(ids) => (
            ids.left.as_ref().ok_or(MetadataError::MissingImageId {
                field: "left_ir_image_id",
            })?,
            ids.right.as_ref().ok_or(MetadataError::MissingImageId {
                field: "right_ir_image_id",
            })?,
            ids.thumbnail.ok_or(MetadataError::MissingImageId {
                field: "thumbnail_image_id",
            })?,
            ids.left_iris_code_aggregate,
            ids.right_iris_code_aggregate,
        ),
    };
    let mut fields: BTreeMap<String, _> = [
        ("left_ir_image_id", json!(left.primary)),
        ("left_ir_multiframe_image_ids", json!(left.multiframe)),
        ("left_iris_code_aggregate_image_ids", json!(left_aggregate)),
        ("right_ir_image_id", json!(right.primary)),
        ("right_ir_multiframe_image_ids", json!(right.multiframe)),
        (
            "right_iris_code_aggregate_image_ids",
            json!(right_aggregate),
        ),
        ("thumbnail_image_id", json!(thumbnail)),
        (
            "orb_public_key_certificate",
            json!(BASE64.encode(info.orb_public_key_certificate)),
        ),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    let timestamp = info
        .capture_start
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let mut hashes = BTreeMap::new();
    for (name, value) in [
        ("signup_id", info.signup_id),
        ("signup_reason", info.signup_reason),
        ("orb_id", info.orb_id),
        ("operator_id", info.operator_id),
        ("timestamp", timestamp.as_str()),
        ("qr_code", info.qr_code),
        ("id_commitment", info.id_commitment),
        ("software_version", info.software_version),
        ("orb_country", info.orb_country),
    ]
    .into_iter()
    .chain(info.device_public_key.map(|key| ("device_public_key", key)))
    {
        let mut salt = [0; 16];
        rng.try_fill_bytes(&mut salt)
            .map_err(MetadataError::Randomness)?;
        let salt = HEXLOWER.encode(&salt);
        let mut hash = Context::new(&SHA256);
        hash.update(value.as_bytes());
        hash.update(salt.as_bytes());
        hashes.insert(
            name,
            hash.finish()
                .as_ref()
                .try_into()
                .expect("SHA-256 produces 32 bytes"),
        );
        fields.insert(name.to_owned(), json!(value));
        fields.insert(format!("{name}_salt"), json!(salt));
    }
    Ok(EncodedMetadata {
        info_json: serde_json::to_vec(&fields)?,
        hashes,
    })
}

#[cfg(test)]
#[path = "../tests/unit/metadata.rs"]
mod tests;
