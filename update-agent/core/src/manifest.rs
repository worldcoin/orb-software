use serde::{de, Deserialize, Serialize};
use tap::TapOptional as _;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("required manifest fields not set: [{}]", .0.join(", "))]
    FieldsNotSet(Vec<&'static str>),
    #[error("manifest contained components with duplicate names: [{}]", .0.join(", "))]
    DuplicateComponents(Vec<String>),
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UpdateKind {
    Full,
    #[default]
    Normal,
}

impl UpdateKind {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub fn is_normal(&self) -> bool {
        matches!(self, Self::Normal)
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct Manifest {
    magic: String,
    #[serde(rename = "type")]
    kind: UpdateKind,
    components: Vec<ManifestComponent>,
}

impl Manifest {
    pub fn builder() -> ManifestBuilder {
        ManifestBuilder::new()
    }

    pub fn magic(&self) -> &str {
        &self.magic
    }

    pub fn kind(&self) -> UpdateKind {
        self.kind
    }

    pub fn is_full_update(&self) -> bool {
        self.kind.is_full()
    }

    pub fn is_normal_update(&self) -> bool {
        self.kind.is_normal()
    }

    pub fn components(&self) -> &[ManifestComponent] {
        &self.components
    }

    /// Unlike [`Self::is_strictly_equal_to()`], this is more relaxed and is agnostic
    /// to the order of components.
    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        // Magic should always be the same after validation.
        if self.magic != other.magic
            || self.kind != other.kind
            || self.components.len() != other.components.len()
        {
            return false;
        }
        let mut this_components_sorted = self.components.clone();
        this_components_sorted.sort_unstable_by(|a, b| a.hash.cmp(&b.hash));
        let mut other_components_sorted = other.components.clone();
        other_components_sorted.sort_unstable_by(|a, b| a.hash.cmp(&b.hash));

        this_components_sorted
            .iter()
            .zip(other_components_sorted.iter())
            .all(|(this_component, other_component)| {
                this_component.is_equivalent_to(other_component)
            })
    }

    /// Unlike [`Self::is_equivalent_to()`], this also requires the ordering of
    /// the components to match.
    pub fn is_strictly_equal_to(&self, other: &Self) -> bool {
        let Self {
            magic,
            kind,
            components,
        } = self;
        *magic == other.magic && *kind == other.kind && *components == other.components
    }
}

/// This code exists to guard against the Manifest implementing PartialEq.
/// Manifest should never implement this directly, because it would encourage
/// overly strict equality comparisions. Use [`Manifest::is_equivalent_to()`]
/// or [`Manifest::is_strictly_equal_to()`] instead of PartialEq to be more
/// explicit.
#[cfg(test)]
#[allow(dead_code)]
mod manifest_should_not_impl_partialeq {
    use super::Manifest;

    trait ManifestShouldNotImplementPartialEq {}

    impl<T: PartialEq> ManifestShouldNotImplementPartialEq for T {}

    impl ManifestShouldNotImplementPartialEq for Manifest {}
}

#[derive(Default)]
pub struct ManifestBuilder {
    pub components: Vec<ManifestComponent>,
    pub kind: UpdateKind,
    pub magic: Option<String>,
}

impl ManifestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn components(self, components: Vec<ManifestComponent>) -> Self {
        Self { components, ..self }
    }

    pub fn kind(self, kind: UpdateKind) -> Self {
        Self { kind, ..self }
    }

    pub fn magic(self, magic: impl Into<String>) -> Self {
        Self {
            magic: Some(magic.into()),
            ..self
        }
    }

    pub fn build(self) -> Result<Manifest, Error> {
        let mut missing_fields = Vec::new();
        let magic = self.magic.tap_none(|| missing_fields.push("magic"));

        if !missing_fields.is_empty() {
            return Err(Error::FieldsNotSet(missing_fields));
        }

        let kind = self.kind;
        let magic = magic.expect("`magic` was verified to contain a value");

        let components = self.components;
        let duplicate_components = find_duplicate_components(&components);
        if !duplicate_components.is_empty() {
            return Err(Error::DuplicateComponents(duplicate_components));
        }

        Ok(Manifest {
            components,
            kind,
            magic,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InstallationPhase {
    #[default]
    Normal,
    Recovery,
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct ManifestComponent {
    pub name: String,
    #[serde(rename = "version-assert")]
    pub version_assert: String,
    #[serde(rename = "version")]
    pub version_upgrade: String,
    pub size: u64,
    #[serde(rename = "hash")]
    pub hash: String,
    pub installation_phase: InstallationPhase,
}

impl ManifestComponent {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version_assert(&self) -> &str {
        &self.version_assert
    }

    pub fn version_upgrade(&self) -> &str {
        &self.version_upgrade
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version_assert == other.version_assert
            && self.version_upgrade == other.version_upgrade
            && self.size == other.size
            && self.hash == other.hash
            && self.installation_phase == other.installation_phase
    }

    pub fn installation_phase(&self) -> InstallationPhase {
        self.installation_phase
    }
}

/// `UncheckedManifest` is a shadow of `Manifest`. It is used as an interim deserialization
/// target inside `Manifest`'s deserialization implementation. `Manifest`'s deserializer then
/// checks if `UncheckedManifest` upholds all its invariants before returning `Manifest`.
#[derive(Clone, Debug, Deserialize)]
pub(super) struct UncheckedManifest {
    magic: String,
    #[serde(rename = "type")]
    kind: UpdateKind,
    components: Vec<ManifestComponent>,
}

#[derive(Debug, thiserror::Error)]
#[error("manifest contained components with duplicate names: {}", .components.join(", "))]
pub struct ManifestHasDuplicateComponents {
    components: Vec<String>,
}

impl<'de> Deserialize<'de> for Manifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let unchecked_manifest = UncheckedManifest::deserialize(deserializer)?;
        unchecked_manifest
            .try_into()
            // Serde throws away the backtraces of the underlying errors, so we must
            // manually create a debug log of the error to save it.
            .map_err(|e| de::Error::custom(format!("{e:?}")))
    }
}

impl TryFrom<UncheckedManifest> for Manifest {
    type Error = ManifestHasDuplicateComponents;

    fn try_from(unchecked_manifest: UncheckedManifest) -> Result<Self, Self::Error> {
        let component_dupes = find_duplicate_components(&unchecked_manifest.components);

        if component_dupes.is_empty() {
            let UncheckedManifest {
                magic,
                kind,
                components,
            } = unchecked_manifest;
            Ok(Manifest {
                magic,
                kind,
                components,
            })
        } else {
            Err(ManifestHasDuplicateComponents {
                components: component_dupes,
            })
        }
    }
}

fn find_duplicate_components(components: &[ManifestComponent]) -> Vec<String> {
    let mut dupes = Vec::new();
    let mut components = components.to_vec();
    components.sort_unstable_by(|comp, other| comp.name.cmp(&other.name));
    let mut iter = components.iter().peekable();
    while let Some(component) = iter.next() {
        match iter.peek() {
            Some(next) if component.name == next.name => {
                dupes.push(component.name.clone());

                // Advance the iterator until we peek a component with a different name
                'advance: while let Some(_) = iter.next() {
                    #[allow(clippy::collapsible_if)]
                    if let Some(next) = iter.peek() {
                        if component.name != next.name {
                            break 'advance;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    dupes
}
