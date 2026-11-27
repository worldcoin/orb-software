use std::collections::{BTreeMap, HashMap, HashSet};

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize, Serializer};
use tap::TapOptional as _;
use tracing::warn;

use crate::{Component, LocalOrRemote};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("required claim fields not set: [{}]", .0.join(", "))]
    ClaimFieldsNotSet(Vec<&'static str>),
    #[error("manifest contained components that lacked sources: [{}]", .0.join(", "))]
    ManifestComponentsWithoutSources(Vec<String>),
    #[error("manifest contained components not listed in system components: [{}]", .0.join(", "))]
    ManifestComponentsNotInSystemComponents(Vec<String>),
    #[error("failed to verify manifest: {0}")]
    ManifestVerification(#[from] crate::signatures::ManifestVerificationError),
    #[error("missing manifest signature")]
    ManifestSignatureMissing,
    #[error("claim contained sources whose name is not a single path component: [{}]", .0.join(", "))]
    SourcesWithInvalidNames(Vec<String>),
    #[error("claim contained sources whose hash is not a sha256 hex digest: [{}]", .0.join(", "))]
    SourcesWithInvalidHashes(Vec<String>),
}

#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MimeType {
    #[serde(rename = "application/octet-stream")]
    OctetStream,
    #[serde(rename = "application/x-xz")]
    XZ,
    #[serde(rename = "application/zstd-bidiff")]
    ZstdBidiff,
}

/// The source of a component.
///
/// `Source` includes the name of the component, its location (whether on the local filesystem or
/// at a remote location, downloadable via HTTPS), its mime type, its size in bytes, and its sha256
/// hash.
///
/// The hash is used to verify that the downloaded binary blob is correct, and is not necessarily
/// the same component as the hash of the final component (as recorded in the manifest).
#[derive(Deserialize, Serialize, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Source {
    pub hash: String,
    pub mime_type: MimeType,
    pub name: String,
    pub size: u64,
    pub url: LocalOrRemote,
}

impl Source {
    pub fn is_local(&self) -> bool {
        self.url.is_local()
    }

    pub fn is_remote(&self) -> bool {
        self.url.is_remote()
    }

    pub fn unique_name(&self) -> String {
        format!("{}-{}", self.name, self.hash)
    }

    /// Whether `name` is a single ordinary path component.
    ///
    /// `sources` is not covered by the manifest signature, and [`Self::unique_name`] is
    /// used as an on-disk file name relative to the downloads directory. A name
    /// carrying separators or `..` would therefore let a tampered claim direct writes
    /// outside of it.
    fn has_valid_name(&self) -> bool {
        !self.name.is_empty()
            && self.name != "."
            && self.name != ".."
            && !self.name.contains(['/', '\\', '\0'])
    }

    /// Whether `hash` is a sha256 digest in hex, i.e. exactly 64 ASCII hex digits.
    ///
    /// Besides being the shape the rest of the agent expects, this keeps the hash half
    /// of [`Self::unique_name`] from contributing path separators of its own.
    fn has_valid_hash(&self) -> bool {
        self.hash.len() == SHA256_HEX_LEN
            && self.hash.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// Length of a sha256 digest in hex characters.
const SHA256_HEX_LEN: usize = 64;

pub struct ClaimBuilder {
    pub manifest: Option<crate::Manifest>,
    pub manifest_raw: Option<String>,
    pub signature: Option<String>,
    pub sources: HashMap<String, Source>,
    pub system_components: Option<crate::Components>,
    pub version: Option<String>,
}

impl ClaimBuilder {
    pub fn new() -> Self {
        Self {
            manifest: None,
            manifest_raw: None,
            signature: None,
            sources: HashMap::new(),
            system_components: None,
            version: None,
        }
    }

    pub fn manifest(self, manifest: crate::Manifest) -> Self {
        Self {
            manifest: Some(manifest),
            ..self
        }
    }

    pub fn manifest_raw(self, manifest_raw: String) -> Self {
        Self {
            manifest_raw: Some(manifest_raw),
            ..self
        }
    }

    pub fn signature(self, signature: impl Into<String>) -> Self {
        Self {
            signature: Some(signature.into()),
            ..self
        }
    }

    pub fn sources(self, sources: HashMap<String, Source>) -> Self {
        Self { sources, ..self }
    }

    pub fn system_components(self, system_components: crate::Components) -> Self {
        Self {
            system_components: Some(system_components),
            ..self
        }
    }

    pub fn version(self, version: impl Into<String>) -> Self {
        Self {
            version: Some(version.into()),
            ..self
        }
    }

    pub fn build(self, manifest_pubkey: &VerifyingKey) -> Result<Claim, Error> {
        let mut missing_fields = Vec::new();
        let manifest = self.manifest.tap_none(|| missing_fields.push("manifest"));
        // not actually a json field
        let manifest_raw = self
            .manifest_raw
            .tap_none(|| missing_fields.push("manifest_raw"));
        let system_components = self
            .system_components
            .tap_none(|| missing_fields.push("system_components"));
        let version = self.version.tap_none(|| missing_fields.push("version"));

        if !missing_fields.is_empty() {
            return Err(Error::ClaimFieldsNotSet(missing_fields));
        }
        let system_components = system_components
            .expect("`system_components` was verified to contain a value");
        let manifest = manifest.expect("`manifest` was verified to contain a value");
        let manifest_raw =
            manifest_raw.expect("`manifest_raw` was verified to contain a value");
        let version = version.expect("`version` was verified to contain a value");

        let sources = self.sources;

        // `sources` is outside the signed manifest, so its strings are validated before
        // anything else consumes them.
        let invalid_names = collect_sources_matching(&sources, |s| !s.has_valid_name());
        if !invalid_names.is_empty() {
            return Err(Error::SourcesWithInvalidNames(invalid_names));
        }
        let invalid_hashes =
            collect_sources_matching(&sources, |s| !s.has_valid_hash());
        if !invalid_hashes.is_empty() {
            return Err(Error::SourcesWithInvalidHashes(invalid_hashes));
        }

        let components_without_sources =
            find_components_without_sources(manifest.components(), &sources);
        if !components_without_sources.is_empty() {
            return Err(Error::ManifestComponentsWithoutSources(
                components_without_sources,
            ));
        }
        let components_not_in_system =
            find_components_not_in_system(&manifest, &system_components);
        if !components_not_in_system.is_empty() {
            return Err(Error::ManifestComponentsNotInSystemComponents(
                components_not_in_system,
            ));
        }

        if cfg!(feature = "skip-manifest-signature-verification") {
            warn!("skipping manifest signature verification due to feature flag");
        } else {
            let Some(ref signature) = self.signature else {
                return Err(Error::ManifestSignatureMissing);
            };
            // "mt" has been often used as a placeholder value for an empty signature
            if signature.is_empty() || signature == "mt" {
                return Err(Error::ManifestSignatureMissing);
            }
            crate::signatures::verify_signature(
                manifest_pubkey,
                signature,
                manifest_raw.as_bytes(),
            )?;
        }

        warn_on_source_name_mismatches(manifest.components(), &sources);

        Ok(Claim {
            manifest,
            signature: self.signature,
            sources,
            system_components,
            version,
        })
    }
}

impl Default for ClaimBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ComponentIter<'a> {
    claim: &'a Claim,
    component_iter: std::slice::Iter<'a, crate::ManifestComponent>,
}

impl<'a> Iterator for ComponentIter<'a> {
    type Item = (&'a crate::ManifestComponent, &'a Source);

    fn next(&mut self) -> Option<Self::Item> {
        self.component_iter
            .next()
            .map(|comp| (comp, &self.claim.sources[&comp.name]))
    }
}

/// For use with serde's [serialize_with] attribute. It ensures
/// deterministic serialization of a hash map.
// Code from https://stackoverflow.com/a/42723390
fn ordered_map<S, K: Ord + Serialize, V: Serialize>(
    value: &HashMap<K, V>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let ordered: BTreeMap<_, _> = value.iter().collect();
    ordered.serialize(serializer)
}

#[derive(Serialize, Debug, Clone)]
pub struct Claim {
    version: String,
    manifest: crate::Manifest,
    #[serde(rename = "manifest-sig")]
    signature: Option<String>,
    #[serde(serialize_with = "ordered_map")]
    sources: HashMap<String, Source>,
    #[serde(serialize_with = "ordered_map")]
    system_components: crate::Components,
}

impl Claim {
    pub fn builder() -> ClaimBuilder {
        ClaimBuilder::new()
    }

    pub fn manifest_components(&self) -> &[crate::ManifestComponent] {
        self.manifest.components()
    }

    pub fn system_components(&self) -> &crate::Components {
        &self.system_components
    }

    pub fn num_components(&self) -> usize {
        self.manifest.components().len()
    }

    pub fn iter_components_with_location(&self) -> ComponentIter<'_> {
        ComponentIter {
            claim: self,
            component_iter: self.manifest.components().iter(),
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn manifest(&self) -> &crate::Manifest {
        &self.manifest
    }

    pub fn signature(&self) -> Option<&str> {
        self.signature.as_deref()
    }

    pub fn sources(&self) -> &HashMap<String, Source> {
        &self.sources
    }

    pub fn full_update_size(&self) -> u64 {
        let sources_size = self.sources.iter().fold(0, |acc, (_, s)| acc + s.size);
        let components_size = self
            .manifest()
            .components()
            .iter()
            .fold(0, |acc, m| acc + m.size);
        sources_size + components_size
    }
}

/// This code exists to guard against the claim implementing serde::Deserialize.
/// Claim should never implement this directly, because it would bypass the important
/// validity checks it performs. Use [`ClaimVerificationContext`] instead.
#[cfg(test)]
#[allow(dead_code)]
mod claim_should_not_impl_deserialize {
    use super::Claim;

    trait ClaimShouldNotImplementDeserialize {}

    impl<'de, T: serde::Deserialize<'de>> ClaimShouldNotImplementDeserialize for T {}

    impl ClaimShouldNotImplementDeserialize for Claim {}
}

/// Finds all components in the manifest that are not listed in system components.
///
/// All manifest components must exist in the system components.
fn find_components_not_in_system(
    manifest: &crate::Manifest,
    system_components: &HashMap<String, Component>,
) -> Vec<String> {
    manifest
        .components()
        .iter()
        .filter_map(|c| {
            if system_components.contains_key(&c.name) {
                None
            } else {
                Some(c.name.clone())
            }
        })
        .collect()
}

/// Collects the `name` of every source matching `predicate`, quoted so that empty names
/// and control characters stay visible in the error message.
fn collect_sources_matching(
    sources: &HashMap<String, Source>,
    predicate: impl Fn(&Source) -> bool,
) -> Vec<String> {
    let mut matching: Vec<String> = sources
        .values()
        .filter(|s| predicate(s))
        .map(|s| format!("{:?}", s.name))
        .collect();
    // `sources` is a hash map, so sort to keep the message deterministic.
    matching.sort_unstable();
    matching
}

/// Logs claims whose `sources` do not line up with the signed manifest.
///
/// Called only after the manifest signature has been verified, so unauthenticated
/// claims cannot generate these warnings. Neither condition is rejected yet: this only
/// collects fleet data for enforcing the binding later. Keys that match no manifest
/// component are attacker-influenced and unvalidated, so only their count is logged.
fn warn_on_source_name_mismatches(
    components: &[crate::ManifestComponent],
    sources: &HashMap<String, Source>,
) {
    let manifest_names: HashSet<&str> =
        components.iter().map(|c| c.name.as_str()).collect();
    let mut extra_sources = 0usize;
    for (key, source) in sources {
        if !manifest_names.contains(key.as_str()) {
            extra_sources += 1;
        } else if source.name != *key {
            warn!(
                manifest_component = %key,
                // `?` so control characters in the unsigned name are escaped
                source_name = ?source.name,
                "claim source name differs from the manifest component it is keyed by"
            );
        }
    }
    if extra_sources > 0 {
        warn!(
            count = extra_sources,
            "claim contained sources entries matching no manifest component"
        );
    }
}

fn find_components_without_sources<V>(
    components: &[crate::ManifestComponent],
    sources: &HashMap<String, V>,
) -> Vec<String> {
    let mut components_without_url = Vec::new();
    for component in components {
        if !sources.contains_key(&component.name) {
            components_without_url.push(component.name.clone());
        }
    }
    components_without_url
}

pub struct ClaimVerificationContext<'a>(pub &'a VerifyingKey);

mod serde_imp {
    use std::collections::HashMap;

    use ed25519_dalek::VerifyingKey;
    use serde::{
        de::{self, DeserializeSeed},
        Deserialize, Serialize,
    };

    use super::{ordered_map, Claim, ClaimVerificationContext, Source};

    impl<'de> DeserializeSeed<'de> for ClaimVerificationContext<'_> {
        type Value = Claim;

        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let unchecked_claim = UncheckedClaim::deserialize(deserializer)?;
            unchecked_claim
                .try_into_claim(self.0)
                // Serde throws away the backtraces of the underlying errors, so we must
                // manually create a debug log of the error to save it.
                .map_err(|e| de::Error::custom(format!("{e:?}")))
        }
    }

    /// `UncheckedClaim` is a shadow of `Claim`. It is used as an interim deserialization target
    /// inside `Claim`'s deserialization implementation. `Claim`'s deserializer then checks if
    /// `UncheckedClaim` upholds all its invariants before returning `Claim`.
    #[derive(Debug, Deserialize, Serialize, Clone)]
    pub struct UncheckedClaim {
        version: String,
        manifest: UncheckedManifest,
        /// Signed sha256 hash of the claim
        #[serde(rename = "manifest-sig")]
        signature: Option<String>,
        #[serde(serialize_with = "ordered_map")]
        pub sources: HashMap<String, Source>,
        #[serde(serialize_with = "ordered_map")]
        system_components: crate::Components,
    }

    #[derive(Debug, Clone)]
    struct UncheckedManifest {
        manifest: crate::Manifest,
        raw: String,
    }

    impl<'de> Deserialize<'de> for UncheckedManifest {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let manifest_raw: Box<serde_json::value::RawValue> =
                Deserialize::deserialize(deserializer)?;
            let manifest = serde_json::from_str(manifest_raw.get())
                .map_err(serde::de::Error::custom)?;

            Ok(Self {
                manifest,
                raw: manifest_raw.get().to_string(),
            })
        }
    }

    impl Serialize for UncheckedManifest {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            // Need to serialize raw bytes verbatim to preserve the exact manifest
            // content that was signed.
            let raw: &serde_json::value::RawValue =
                serde_json::from_str(&self.raw).map_err(serde::ser::Error::custom)?;
            raw.serialize(serializer)
        }
    }

    #[cfg(test)]
    mod test {
        use crate::{Manifest, ManifestComponent};

        use super::*;

        #[test]
        fn test_unchecked_manifest_round_trips() {
            let manifest = Manifest::builder()
                .magic("magic is required to avoid panic")
                .components(vec![ManifestComponent {
                    name: String::from("example"),
                    version_assert: String::new(),
                    version_upgrade: String::new(),
                    size: 1337,
                    hash: String::new(),
                    installation_phase: crate::manifest::InstallationPhase::Normal,
                }])
                .kind(crate::manifest::UpdateKind::Full)
                .build()
                .expect("failed to build manifest");

            let serialized = serde_json::to_vec(&manifest)
                .expect("valid manifest should always serialize");

            let deserialized_unchecked: UncheckedManifest = serde_json::from_slice(
                &serialized,
            )
            .expect("valid manifests should always deserialize into UncheckedManifest");
            assert!(
                deserialized_unchecked
                    .manifest
                    .is_strictly_equal_to(&manifest),
                "deserialized manifest didn't match original"
            );
            let re_serialized = serde_json::to_vec(&deserialized_unchecked)
                .expect("valid manifests should always serialize");
            assert_eq!(
                re_serialized, serialized,
                "re-serialized manifest doesn't match original serialized"
            );
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub(crate) enum ClaimDeserializationError {
        #[error("claim is invalid")]
        ClaimInvalid(#[from] crate::claim::Error),
    }

    impl UncheckedClaim {
        fn try_into_claim(
            self,
            manifest_pubkey: &VerifyingKey,
        ) -> Result<Claim, ClaimDeserializationError> {
            let UncheckedClaim {
                version,
                manifest,
                signature,
                sources,
                system_components,
            } = self;

            let builder = Claim::builder()
                .version(version)
                .manifest(manifest.manifest)
                .manifest_raw(manifest.raw)
                .sources(sources)
                .system_components(system_components);
            let builder = if let Some(signature) = signature {
                builder.signature(signature)
            } else {
                builder
            };
            builder.build(manifest_pubkey).map_err(Into::into)
        }
    }
}
pub use serde_imp::UncheckedClaim;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use base64::Engine as _;
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde::de::DeserializeSeed as _;

    use super::*;

    const VALID_HASH: &str =
        "bc4c24181ed3ce6666444deeb95e1f61940bffee70dd13972beb331f5d111e9b";

    fn source(name: &str, hash: &str) -> Source {
        Source {
            hash: hash.to_owned(),
            mime_type: MimeType::OctetStream,
            name: name.to_owned(),
            size: 4,
            url: LocalOrRemote::Local(PathBuf::from("blob")),
        }
    }

    #[test]
    fn ordinary_source_names_are_accepted() {
        for name in ["rootfs", "main_mcu", "edge-app", "foo.bar"] {
            assert!(
                source(name, VALID_HASH).has_valid_name(),
                "name `{name}` should be a valid component name"
            );
        }
    }

    #[test]
    fn source_names_that_are_not_a_single_path_component_are_rejected() {
        for name in ["../x", "a/b", "/etc/x", "..", ".", "", "a\\b", "a\0b"] {
            assert!(
                !source(name, VALID_HASH).has_valid_name(),
                "name `{name:?}` should be rejected as a component name"
            );
        }
    }

    #[test]
    fn sha256_hex_hashes_are_accepted() {
        assert!(source("rootfs", VALID_HASH).has_valid_hash());
        assert!(source("rootfs", &VALID_HASH.to_uppercase()).has_valid_hash());
    }

    #[test]
    fn hashes_that_are_not_sha256_hex_are_rejected() {
        let too_short = &VALID_HASH[..63];
        let too_long = format!("{VALID_HASH}a");
        let with_slash = format!("../{}", &VALID_HASH[3..]);
        let with_dots = format!("..{}", &VALID_HASH[2..]);
        let with_non_hex = format!("G{}", &VALID_HASH[1..]);
        for hash in [
            "",
            too_short,
            too_long.as_str(),
            with_slash.as_str(),
            with_dots.as_str(),
            with_non_hex.as_str(),
        ] {
            assert!(
                !source("rootfs", hash).has_valid_hash(),
                "hash `{hash}` should be rejected"
            );
        }
    }

    /// Fixed, test-only signing key; it is never a production key.
    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn manifest_json(component_name: &str) -> String {
        format!(
            r#"{{"magic":"test-magic","type":"normal","components":[{{"name":"{component_name}","version-assert":"1.0.0","version":"1.0.1","size":4,"hash":"{VALID_HASH}","installation_phase":"normal"}}]}}"#
        )
    }

    fn sources_json(key: &str, name: &str) -> String {
        format!(
            r#"{{"{key}":{{"hash":"{VALID_HASH}","mime_type":"application/octet-stream","name":"{name}","size":4,"url":"https://example.com/blob"}}}}"#
        )
    }

    fn system_components_json(key: &str) -> String {
        format!(
            r#"{{"{key}":{{"type":"raw","value":{{"device":"emmc","offset":0,"size":4,"redundancy":"single"}}}}}}"#
        )
    }

    /// Assembles a claim, signing `manifest_raw` and embedding it verbatim: the
    /// signature covers exactly the raw JSON substring `RawValue` captures, so the
    /// bytes signed and the bytes embedded must be identical.
    fn claim_json(
        manifest_raw: &str,
        sources: &str,
        system_components: &str,
    ) -> String {
        let signature = base64::prelude::BASE64_STANDARD
            .encode(signing_key().sign(manifest_raw.as_bytes()).to_bytes());
        format!(
            r#"{{"version":"1.0.1","manifest":{manifest_raw},"manifest-sig":"{signature}","sources":{sources},"system_components":{system_components}}}"#
        )
    }

    fn parse_claim(json: &str) -> Result<Claim, serde_json::Error> {
        let pubkey = signing_key().verifying_key();
        let mut de = serde_json::Deserializer::from_str(json);
        ClaimVerificationContext(&pubkey).deserialize(&mut de)
    }

    #[test]
    fn honest_signed_claim_parses() {
        let json = claim_json(
            &manifest_json("rootfs"),
            &sources_json("rootfs", "rootfs"),
            &system_components_json("rootfs"),
        );
        let claim = parse_claim(&json).expect("honest claim should parse");
        assert_eq!(claim.num_components(), 1);
        assert_eq!(claim.sources()["rootfs"].name, "rootfs");
    }

    #[test]
    fn signed_claim_with_traversing_source_name_is_rejected() {
        let json = claim_json(
            &manifest_json("rootfs"),
            &sources_json("rootfs", "../../etc/cron.d/x"),
            &system_components_json("rootfs"),
        );
        let err = parse_claim(&json)
            .expect_err("traversing source name should be rejected")
            .to_string();
        assert!(
            err.contains("SourcesWithInvalidNames"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn signed_claim_with_non_hex_source_hash_is_rejected() {
        let sources = r#"{"rootfs":{"hash":"deadbeef","mime_type":"application/octet-stream","name":"rootfs","size":4,"url":"https://example.com/blob"}}"#;
        let json = claim_json(
            &manifest_json("rootfs"),
            sources,
            &system_components_json("rootfs"),
        );
        let err = parse_claim(&json)
            .expect_err("short source hash should be rejected")
            .to_string();
        assert!(
            err.contains("SourcesWithInvalidHashes"),
            "unexpected error: {err}"
        );
    }

    /// Control for the two tests above: they must fail on the source strings, not
    /// because the fixture's signature was bogus all along.
    #[cfg(not(feature = "skip-manifest-signature-verification"))]
    #[test]
    fn claim_with_tampered_manifest_is_rejected() {
        let signed = manifest_json("rootfs");
        let tampered = signed.replace("test-magic", "othermagic");
        assert_ne!(signed, tampered, "tampering must change the manifest");
        let json = claim_json(
            &signed,
            &sources_json("rootfs", "rootfs"),
            &system_components_json("rootfs"),
        )
        .replace(&signed, &tampered);
        let err = parse_claim(&json)
            .expect_err("tampered manifest should be rejected")
            .to_string();
        assert!(
            err.contains("ManifestVerification"),
            "unexpected error: {err}"
        );
    }
}
