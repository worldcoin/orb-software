use std::{collections::HashMap, iter::Extend};

use serde::{Deserialize, Serialize};

use crate::{
    components::{
        Location::{self, *},
        Redundancy::{self, *},
    },
    slot::Slot,
};

pub type VersionsLegacy = Versions;

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct SlotReleases {
    pub(crate) slot_a: String,
    pub(crate) slot_b: String,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Versions {
    pub(crate) releases: SlotReleases,
    pub(crate) slot_a: VersionGroup,
    pub(crate) slot_b: VersionGroup,
    pub(crate) singles: VersionGroup,
}

#[derive(Deserialize, Serialize, Debug, Default, PartialEq, Eq)]
pub struct VersionGroup {
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub(crate) jetson: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub(crate) mcu: HashMap<String, String>,
}

impl VersionGroup {
    pub fn is_empty(&self) -> bool {
        self.jetson.is_empty() & self.mcu.is_empty()
    }

    pub fn len(&self) -> usize {
        self.jetson.len() + self.mcu.len()
    }

    pub fn flatten_components(&self) -> HashMap<&str, std::borrow::Cow<str>> {
        let mut map = HashMap::with_capacity(self.len());

        map.extend(self.jetson.iter().map(|(k, v)| (&**k, v.into())));
        map.extend(self.mcu.iter().map(|(k, v)| (&**k, v.into())));
        map
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.jetson.contains_key(key) || self.mcu.contains_key(key)
    }

    fn update_component(&mut self, component: &str, version: &str) -> Option<String> {
        if self.jetson.contains_key(component) {
            self.jetson
                .insert(component.to_string(), version.to_string())
        } else if self.mcu.contains_key(component) {
            self.mcu.insert(component.to_string(), version.to_string())
        } else {
            None
        }
    }

    /// If present, returns a triple of `(name, version, location)`, with `location` being `Jetson`
    /// or `Mcu`, `name` being the components name, and `version` its version.
    fn get_component(&self, name: &str) -> Option<(&str, &str, Location)> {
        self.jetson
            .get_key_value(name)
            .map(|(name, version)| (&**name, &**version, Jetson))
            .or_else(|| {
                self.mcu
                    .get_key_value(name)
                    .map(|(name, version)| (&**name, &**version, Mcu))
            })
    }
}

impl Versions {
    pub fn collect_versions(&self, slot: Slot) -> HashMap<&str, std::borrow::Cow<str>> {
        let version_group = match slot {
            Slot::A => &self.slot_a,
            Slot::B => &self.slot_b,
        };
        let mut map = HashMap::with_capacity(self.slot_a.len() + self.singles.len());
        map.extend(version_group.flatten_components());
        map.extend(self.singles.flatten_components());
        map
    }

    pub fn is_empty(&self) -> bool {
        self.slot_a.is_empty() && self.singles.is_empty()
    }

    pub fn len(&self) -> usize {
        self.slot_a.len() + self.singles.len()
    }

    pub fn update_component(
        &mut self,
        slot: Slot,
        component: &str,
        version: &str,
    ) -> Option<String> {
        let redundant_group = match slot {
            Slot::A => &mut self.slot_a,
            Slot::B => &mut self.slot_b,
        };

        redundant_group
            .update_component(component, version)
            .or_else(|| self.singles.update_component(component, version))
    }

    pub fn copy_component(
        &mut self,
        src_slot: Slot,
        component: &str,
    ) -> Option<String> {
        let (src_group, dst_group) = match src_slot {
            Slot::A => (&self.slot_a, &mut self.slot_b),
            Slot::B => (&self.slot_b, &mut self.slot_a),
        };

        src_group
            .get_component(component)
            .and_then(|(_, src_version, _)| {
                dst_group.update_component(component, src_version)
            })
    }

    pub fn update_release(&mut self, slot: Slot, release: String) {
        match slot {
            Slot::A => self.releases.slot_a = release,
            Slot::B => self.releases.slot_b = release,
        }
    }
}
