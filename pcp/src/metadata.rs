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
///
/// `Option` fields are omitted together with any salt and hash when absent.
/// Apart from the version-dependent device key, only PCP 2.9 migrations, whose
/// sources may predate these fields, accept absent values.
pub struct PackageInfo<'a> {
    pub signup_id: &'a str,
    pub signup_reason: &'a str,
    pub orb_id: &'a str,
    pub operator_id: &'a str,
    pub capture_start: SystemTime,
    pub qr_code: Option<&'a str>,
    pub id_commitment: Option<&'a str>,
    pub software_version: Option<&'a str>,
    pub orb_country: Option<&'a str>,
    /// Raw certificate bytes, encoded as padded standard Base64.
    pub orb_public_key_certificate: Option<&'a [u8]>,
    pub device_public_key: Option<&'a str>,
}

pub(crate) struct IrisImageIds<'a> {
    pub primary: Option<&'a str>,
    pub multiframe: &'a [&'a str],
}

/// Availability is distinct from redaction. Both eye groups are required;
/// absent groups return an error. Absent primary and thumbnail IDs are omitted;
/// the builder decides which versions accept that.
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
/// `src_signup_id` is written unsalted and unhashed; `migration.pb` covers it.
pub(crate) fn encode(
    info: &PackageInfo<'_>,
    images: &ImageIdPolicy<'_>,
    src_signup_id: Option<&str>,
    rng: &mut (impl RngCore + CryptoRng),
) -> Result<EncodedMetadata, MetadataError> {
    let empty_eye = IrisImageIds {
        primary: Some(""),
        multiframe: &[],
    };
    let (left, right, thumbnail, left_aggregate, right_aggregate) = match images {
        ImageIdPolicy::Redacted => (&empty_eye, &empty_eye, Some(""), &[][..], &[][..]),
        ImageIdPolicy::Included(ids) => (
            ids.left.as_ref().ok_or(MetadataError::MissingImageId {
                field: "left_ir_image_id",
            })?,
            ids.right.as_ref().ok_or(MetadataError::MissingImageId {
                field: "right_ir_image_id",
            })?,
            ids.thumbnail,
            ids.left_iris_code_aggregate,
            ids.right_iris_code_aggregate,
        ),
    };
    let mut fields: BTreeMap<String, _> = [
        ("left_ir_multiframe_image_ids", json!(left.multiframe)),
        ("left_iris_code_aggregate_image_ids", json!(left_aggregate)),
        ("right_ir_multiframe_image_ids", json!(right.multiframe)),
        (
            "right_iris_code_aggregate_image_ids",
            json!(right_aggregate),
        ),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    for (name, value) in [
        ("left_ir_image_id", left.primary),
        ("right_ir_image_id", right.primary),
        ("thumbnail_image_id", thumbnail),
        ("src_signup_id", src_signup_id),
    ] {
        if let Some(value) = value {
            fields.insert(name.to_owned(), json!(value));
        }
    }
    if let Some(certificate) = info.orb_public_key_certificate {
        fields.insert(
            "orb_public_key_certificate".to_owned(),
            json!(BASE64.encode(certificate)),
        );
    }
    let timestamp = info
        .capture_start
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let mut hashes = BTreeMap::new();
    // Salt generation order is part of deterministic compatibility with seeded callers.
    for (name, value) in [
        ("signup_id", Some(info.signup_id)),
        ("signup_reason", Some(info.signup_reason)),
        ("orb_id", Some(info.orb_id)),
        ("operator_id", Some(info.operator_id)),
        ("timestamp", Some(timestamp.as_str())),
        ("qr_code", info.qr_code),
        ("id_commitment", info.id_commitment),
        ("software_version", info.software_version),
        ("orb_country", info.orb_country),
        ("device_public_key", info.device_public_key),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.map(|value| (name, value)))
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
