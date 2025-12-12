#![allow(dead_code)]

use crate::network_manager::WifiProfile;
use color_eyre::Result;
use dashmap::{mapref::one::Ref, DashMap};
use std::{collections::HashMap, sync::Arc};

use orb_secure_storage_ca::Client as SsClient;

const STORAGE_KEY: &str = "nm-profiles-v1";

pub struct ProfileStore {
    profiles: Arc<DashMap<String, WifiProfile>>,
    client: SsClient, // TODO: Allow mocking
}

impl ProfileStore {
    const FILENAME: &str = "nmprofiles";

    pub async fn new() -> Result<Self> {
        Ok(Self {
            profiles: Arc::new(DashMap::new()),
            client: SsClient::new()?,
        })
    }

    pub async fn import(&mut self) -> Result<()> {
        // TODO: Handle changing euid to less priviledged user

        let bytes = self.client.get(STORAGE_KEY)?;

        let profiles: HashMap<String, WifiProfile> = serde_json::from_slice(&bytes)?;

        for (key, value) in profiles {
            self.profiles.insert(key, value);
        }

        Ok(())
    }

    pub async fn commit(&mut self) -> Result<()> {
        // TODO: Handle changing euid to less priviledged user
        let profiles: HashMap<_, _> = self
            .profiles
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        let json = serde_json::to_vec(&profiles)?;
        self.client.put(STORAGE_KEY, &json)?;

        Ok(())
    }

    pub fn insert(&self, profile: WifiProfile) {
        self.profiles.insert(profile.ssid.clone(), profile);
    }

    pub fn remove(&self, ssid: &str) -> Option<WifiProfile> {
        self.profiles.remove(ssid).map(|(_, value)| value)
    }

    pub fn get(&self, ssid: &str) -> Option<Ref<'_, String, WifiProfile>> {
        self.profiles.get(ssid)
    }

    pub fn values(&self) -> Vec<WifiProfile> {
        self.profiles.iter().map(|x| x.value().clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_manager::WifiSec;
    use async_tempfile::TempDir;
    use secrecy::SecretVec;
    use std::path::Path;

    const TEST_KEY: [u8; 32] = [0x42; 32];

    // Helper to build a ProfileStore with a static key, no KeyMaterial needed.
    fn make_store(dir: &Path) -> ProfileStore {
        ProfileStore {
            key: Ok(SecretVec::new(TEST_KEY.to_vec())),
            store_path: dir.to_path_buf(),
            profiles: Arc::new(DashMap::new()),
        }
    }

    #[tokio::test]
    async fn it_imports_and_commits_wifi_profiles() {
        // Arrange
        let tmpdir = TempDir::new().await.unwrap();
        let tmpdirpath = tmpdir.to_path_buf();

        let expected = [
            WifiProfile {
                id: "test-ssid1".into(),
                ssid: "test-ssid1".into(),
                uuid: String::new(),
                sec: WifiSec::Wpa2Psk,
                psk: "12345678".into(),
                autoconnect: true,
                priority: 0,
                hidden: false,
                path: String::new(),
            },
            WifiProfile {
                id: "test-ssid2".into(),
                ssid: "test-ssid2".into(),
                uuid: String::new(),
                sec: WifiSec::Wpa3Sae,
                psk: "12345678".into(),
                autoconnect: true,
                priority: 1,
                hidden: false,
                path: String::new(),
            },
            WifiProfile {
                id: "test-ssid3".into(),
                ssid: "test-ssid3".into(),
                uuid: String::new(),
                sec: WifiSec::Wpa2Psk,
                psk: "12345678".into(),
                autoconnect: true,
                priority: 2,
                hidden: false,
                path: String::new(),
            },
        ];

        let store = make_store(&tmpdirpath);

        for profile in &expected {
            store.insert(profile.clone());
        }

        // Act
        store.commit().await.unwrap();

        // Second store: load from disk and import
        let store = make_store(&tmpdirpath);
        store.import().await.unwrap();

        // Assert
        let mut actual: Vec<_> = store.values();
        actual.sort_by_key(|p| p.priority);
        assert_eq!(actual, expected);

        // Act: remove some profiles
        store.remove("test-ssid2");
        store.remove("test-ssid3");
        store.commit().await.unwrap();

        // Assert
        let store = make_store(&tmpdirpath);
        store.import().await.unwrap();

        let actual: Vec<_> = store.values();
        assert_eq!(actual, vec![expected[0].clone()]);
    }
}
