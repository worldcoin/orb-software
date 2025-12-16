use crate::{network_manager::WifiProfile, secure_storage::SecureStorage};
use color_eyre::Result;
use dashmap::DashMap;
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;

pub struct ProfileStore {
    secure_storage: SecureStorage,
    cancel_token: CancellationToken,
    profiles: Arc<DashMap<String, WifiProfile>>,
}

impl Drop for ProfileStore {
    fn drop(&mut self) {
        self.cancel_token.cancel();
    }
}

impl ProfileStore {
    const KEY: &str = "nmprofiles";

    pub fn new(connd_exe_path: impl AsRef<Path> + 'static, in_memory: bool) -> Self {
        let cancel_token = CancellationToken::new();

        Self {
            secure_storage: SecureStorage::new(
                connd_exe_path,
                in_memory,
                cancel_token.clone(),
            ),
            profiles: Arc::new(DashMap::new()),
            cancel_token,
        }
    }

    pub async fn import(&self) -> Result<()> {
        let bytes = self.secure_storage.get(Self::KEY.into()).await?;
        let profiles: HashMap<String, WifiProfile> =
            ciborium::de::from_reader(bytes.as_slice())?;

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

        self.secure_storage.put(Self::KEY.into(), bytes).await?;

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
