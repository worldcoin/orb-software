use crate::{network_manager::WifiProfile, secure_storage::SecureStorage};
use color_eyre::{eyre::Context, Result};
use dashmap::DashMap;
use std::{collections::HashMap, sync::Arc};

pub struct ProfileStore {
    secure_storage: SecureStorage,
    profiles: Arc<DashMap<String, WifiProfile>>,
}

impl ProfileStore {
    const KEY: &str = "nmprofiles";

    pub fn new(secure_storage: SecureStorage) -> Self {
        Self {
            secure_storage,
            profiles: Arc::new(DashMap::new()),
        }
    }

    pub async fn import(&self) -> Result<()> {
        let bytes = self
            .secure_storage
            .get(Self::KEY.into())
            .await
            .wrap_err("failed trying to import from secure storage")?;

        let profiles: HashMap<String, WifiProfile> = if bytes.is_empty() {
            HashMap::default()
        } else {
            ciborium::de::from_reader(bytes.as_slice())?
        };

        for (key, value) in profiles {
            self.profiles.insert(key, value);
        }

        Ok(())
    }

    pub async fn commit(&self) -> Result<()> {
        let profiles: HashMap<_, _> = self
            .profiles
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&profiles, &mut bytes)?;

        self.secure_storage
            .put(Self::KEY.into(), bytes)
            .await
            .wrap_err("failed trying to commit to secure storage")?;

        Ok(())
    }

    pub fn insert(&self, profile: WifiProfile) {
        self.profiles.insert(profile.ssid.clone(), profile);
    }

    pub fn remove(&self, ssid: &str) -> Option<WifiProfile> {
        self.profiles.remove(ssid).map(|(_, value)| value)
    }

    pub fn values(&self) -> Vec<WifiProfile> {
        self.profiles.iter().map(|x| x.value().clone()).collect()
    }
}
