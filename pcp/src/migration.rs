//! PCP 2.9 migration provenance (`migration.pb`) and preserved `legacy/` files.
//!
//! Only TEE migrations supply these, always together: the legacy files are the
//! migration's source. Orb-captured 2.9 packages contain neither.
//! The builder owns every emitted name: callers supply bytes for the fixed
//! legacy inventory, never paths. Legacy files are copied verbatim and are not
//! covered by the new manifest; their original `hashes.json` and `hashes.sign`
//! authenticate them. Nothing here decodes, verifies or re-encodes source bytes.

use orb_pcp_defs::{prost::Message, v1::Migration};

/// TEE-supplied lineage and execution metadata, encoded as `migration.pb`.
/// `src_signup_id` is also written as the unsalted `info.json` `src_signup_id`.
pub struct MigrationProvenance<'a> {
    /// Release version of the TEE software performing the migration.
    pub tee_version: &'a str,
    /// The source package's `signup_id`.
    pub src_signup_id: &'a str,
    pub source_pcp_version: &'a str,
    /// Unix seconds; independent of the preserved capture timestamp.
    pub migrated_ts: u64,
    pub biometric_pipeline_version: &'a str,
    pub legacy: LegacyArtifacts<'a>,
}

/// Exact source bytes for the closed `legacy/` inventory.
/// `None` omits a file the source lacked; `Some(&[])` preserves an empty file.
pub struct LegacyArtifacts<'a> {
    pub hashes_json: &'a [u8],
    pub hashes_sign: &'a [u8],
    pub face_embeddings_json: Option<&'a [u8]>,
    pub iris_codes_json: Option<&'a [u8]>,
    pub iris_code_shares_json: [Option<&'a [u8]>; 3],
    pub di_iris_embeddings_pb: Option<&'a [u8]>,
    pub di_iris_embeddings_shares_pb: [Option<&'a [u8]>; 3],
}

impl MigrationProvenance<'_> {
    /// Returns the first empty required field.
    pub(crate) fn empty_field(&self) -> Option<&'static str> {
        [
            ("tee_version", self.tee_version.as_bytes()),
            ("src_signup_id", self.src_signup_id.as_bytes()),
            ("source_pcp_version", self.source_pcp_version.as_bytes()),
            (
                "biometric_pipeline_version",
                self.biometric_pipeline_version.as_bytes(),
            ),
            ("legacy/hashes.json", self.legacy.hashes_json),
            ("legacy/hashes.sign", self.legacy.hashes_sign),
        ]
        .into_iter()
        .find_map(|(field, value)| value.is_empty().then_some(field))
    }

    /// Binary protobuf with every field present; the manifest hashes these bytes.
    pub(crate) fn encode(&self) -> Vec<u8> {
        Migration {
            tee_version: Some(self.tee_version.to_owned()),
            src_signup_id: Some(self.src_signup_id.to_owned()),
            source_pcp_version: Some(self.source_pcp_version.to_owned()),
            migrated_ts: Some(self.migrated_ts),
            biometric_pipeline_version: Some(
                self.biometric_pipeline_version.to_owned(),
            ),
        }
        .encode_to_vec()
    }
}

impl<'a> LegacyArtifacts<'a> {
    /// Present files in the same relative order as their top-level counterparts.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&'static str, &'a [u8])> {
        let [iris_0, iris_1, iris_2] = self.iris_code_shares_json;
        let [di_0, di_1, di_2] = self.di_iris_embeddings_shares_pb;
        [
            ("legacy/face_embeddings.json", self.face_embeddings_json),
            ("legacy/iris_codes.json", self.iris_codes_json),
            ("legacy/iris_code_shares_0.json", iris_0),
            ("legacy/iris_code_shares_1.json", iris_1),
            ("legacy/iris_code_shares_2.json", iris_2),
            ("legacy/di_iris_embeddings.pb", self.di_iris_embeddings_pb),
            ("legacy/di_iris_embeddings_shares_0.pb", di_0),
            ("legacy/di_iris_embeddings_shares_1.pb", di_1),
            ("legacy/di_iris_embeddings_shares_2.pb", di_2),
            ("legacy/hashes.sign", Some(self.hashes_sign)),
            ("legacy/hashes.json", Some(self.hashes_json)),
        ]
        .into_iter()
        .filter_map(|(name, bytes)| bytes.map(|bytes| (name, bytes)))
    }
}

#[cfg(test)]
#[path = "../tests/unit/migration.rs"]
mod tests;
