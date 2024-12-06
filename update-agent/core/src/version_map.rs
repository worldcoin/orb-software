use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    versions::{SlotReleases, VersionGroup, VersionsLegacy},
    Component, ManifestComponent, Slot,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum SlotVersion {
    Single {
        version: String,
    },
    Redundant {
        version_a: Option<String>,
        version_b: Option<String>,
    },
}

impl SlotVersion {
    fn new_single(version: impl ToString) -> Self {
        Self::Single {
            version: version.to_string(),
        }
    }

    fn new_redundant_with_slot(version: impl ToString, slot: Slot) -> Self {
        let version = Some(version.to_string());
        match slot {
            Slot::A => Self::Redundant {
                version_a: version,
                version_b: None,
            },
            Slot::B => Self::Redundant {
                version_a: None,
                version_b: version,
            },
        }
    }

    fn mirror_redundant(&mut self, slot: Slot) -> bool {
        match (slot, self) {
            (_, SlotVersion::Single { .. }) => false,
            (
                Slot::A,
                SlotVersion::Redundant {
                    version_a,
                    version_b,
                },
            ) => {
                version_a.clone_from(version_b);
                true
            }
            (
                Slot::B,
                SlotVersion::Redundant {
                    version_a,
                    version_b,
                },
            ) => {
                version_b.clone_from(version_a);
                true
            }
        }
    }

    fn update_redundant_with_slot(self, version: impl ToString, slot: Slot) -> Self {
        match (slot, self) {
            (Slot::A, Self::Redundant { version_b, .. }) => Self::Redundant {
                version_a: Some(version.to_string()),
                version_b,
            },
            (Slot::B, Self::Redundant { version_a, .. }) => Self::Redundant {
                version_a,
                version_b: Some(version.to_string()),
            },
            _ => Self::new_redundant_with_slot(version, slot),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Releases {
    slot_a: Option<String>,
    slot_b: Option<String>,
    recovery: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct VersionMap {
    releases: Releases,
    components: HashMap<String, SlotVersion>,
}

pub struct ComponentIter<'a> {
    inner: Box<dyn Iterator<Item = (&'a String, &'a SlotVersion)> + 'a>,
}

impl<'a> Iterator for ComponentIter<'a> {
    type Item = (&'a str, &'a SlotVersion);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(n, c)| (n.as_str(), c))
    }
}

impl VersionMap {
    pub fn get_slot_a(&self) -> Option<&str> {
        self.releases.slot_a.as_deref()
    }

    pub fn get_slot_b(&self) -> Option<&str> {
        self.releases.slot_b.as_deref()
    }

    pub fn components(&self) -> ComponentIter<'_> {
        ComponentIter {
            inner: Box::new(self.components.iter()),
        }
    }

    pub fn slot_version(&self, name: &str) -> Option<&SlotVersion> {
        self.components.get(name)
    }

    pub fn mirror_redundant_component_version(
        &mut self,
        name: &str,
        target_slot: Slot,
    ) -> bool {
        let Some(slot_version) = self.components.get_mut(name) else {
            return false;
        };
        slot_version.mirror_redundant(target_slot)
    }

    pub fn set_component(
        &mut self,
        target_slot: Slot,
        manifest: &ManifestComponent,
        system_component: &Component,
    ) {
        let new_version = manifest.version_upgrade();
        self.components
            .entry(manifest.name().to_owned())
            .and_modify(|slot_version| {
                if system_component.is_redundant() {
                    let _ = std::mem::replace(
                        slot_version,
                        slot_version
                            .clone()
                            .update_redundant_with_slot(new_version, target_slot),
                    );
                } else {
                    let _ = std::mem::replace(
                        slot_version,
                        SlotVersion::new_single(new_version),
                    );
                }
            })
            .or_insert_with(|| {
                info!(
                    "component `{}` does not yet exist in map; inserting",
                    manifest.name()
                );
                if system_component.is_redundant() {
                    SlotVersion::new_redundant_with_slot(new_version, target_slot)
                } else {
                    SlotVersion::new_single(new_version)
                }
            });
    }

    pub fn set_slot_version(&mut self, version: &str, target_slot: Slot) {
        match target_slot {
            Slot::A => self.releases.slot_a.replace(version.to_string()),
            Slot::B => self.releases.slot_b.replace(version.to_string()),
        };
    }

    pub fn set_recovery_version(&mut self, version: &str) {
        self.releases.recovery.replace(version.to_string());
    }

    /// Constructs a VersionMap from a legacy version representation.
    pub fn from_legacy(legacy: &VersionsLegacy) -> Self {
        let releases = Releases {
            slot_a: Some(legacy.releases.slot_a.clone()),
            slot_b: Some(legacy.releases.slot_b.clone()),
            recovery: None,
        };
        let mut components = HashMap::new();

        for (name, version) in chain_group(&legacy.singles) {
            if components
                .insert(
                    name.to_owned(),
                    SlotVersion::Single {
                        version: version.to_owned(),
                    },
                )
                .is_some()
            {
                warn!(
                    "while copying legacy single component: `{name}` was already present when \
                     inserting into map"
                );
            }
        }

        for (name, version) in chain_group(&legacy.slot_a) {
            if components
                .insert(
                    name.to_owned(),
                    SlotVersion::Redundant {
                        version_a: Some(version.to_owned()),
                        version_b: None,
                    },
                )
                .is_some()
            {
                warn!(
                    "while copying legacy slot_a component: `{name}` was already present when \
                     inserting into map"
                );
            }
        }

        for (name, version) in chain_group(&legacy.slot_b) {
            components
                .entry(name.to_owned())
                .and_modify(|slot_version| match slot_version {
                    SlotVersion::Single { .. } => warn!(
                        "while copying legacy slot_b component: {name} already present as single \
                         slotted component"
                    ),

                    SlotVersion::Redundant { ref mut version_b, .. } => {
                        if version_b.replace(version.to_owned()).is_some() {
                            warn!(
                                "while copying legacy slot_b component: `{name}` already had \
                                 version b set"
                            );
                        }
                    }
                })
                .or_insert_with(|| SlotVersion::Redundant {
                        version_a: None,
                        version_b: Some(version.to_owned()),
                });
        }

        Self {
            releases,
            components,
        }
    }

    pub fn to_legacy(&self) -> VersionsLegacy {
        let mut slot_a = VersionGroup::default();
        let mut slot_b = VersionGroup::default();
        let mut singles = VersionGroup::default();
        for (name, slot_version) in self.components.iter() {
            match &slot_version {
                SlotVersion::Single { version } => {
                    insert_jetson_or_mcu(&mut singles, name, version)
                }
                SlotVersion::Redundant {
                    version_a,
                    version_b,
                } => {
                    if let Some(version_a) = version_a {
                        insert_jetson_or_mcu(&mut slot_a, name, version_a);
                    }
                    if let Some(version_b) = version_b {
                        insert_jetson_or_mcu(&mut slot_b, name, version_b);
                    }
                }
            }
        }
        VersionsLegacy {
            releases: SlotReleases {
                slot_a: self.releases.slot_a.clone().unwrap_or_default(),
                slot_b: self.releases.slot_b.clone().unwrap_or_default(),
            },
            slot_a,
            slot_b,
            singles,
        }
    }
}

fn insert_jetson_or_mcu(group: &mut VersionGroup, name: &str, version: &str) {
    let name = name.to_string();
    let version = version.to_string();
    match &*name {
        "mainboard" | "security" => group.mcu.insert(name, version),
        _ => group.jetson.insert(name, version),
    };
}

fn chain_group(group: &VersionGroup) -> impl Iterator<Item = (&str, &str)> + '_ {
    group
        .jetson
        .iter()
        .chain(group.mcu.iter())
        .map(|(k, v)| (k.as_str(), v.as_str()))
}
