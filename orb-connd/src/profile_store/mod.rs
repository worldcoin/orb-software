use crate::{key_material::KeyMaterial, network_manager::WifiProfile};
use chacha20poly1305::{aead::Aead, AeadCore, Key, KeyInit, XChaCha20Poly1305, XNonce};
use color_eyre::{
    eyre::{bail, ensure, eyre},
    Result,
};
use dashmap::DashMap;
use rand::rngs::OsRng;
use secrecy::{ExposeSecret, SecretVec};
use std::{collections::HashMap, io::ErrorKind, path::PathBuf, sync::Arc};
use tokio::fs;

pub struct ProfileStore {
    key: Result<SecretVec<u8>>,
    store_path: PathBuf,
    profiles: Arc<DashMap<String, WifiProfile>>,
}

impl ProfileStore {
    const FILENAME: &str = "nmprofiles";

    pub async fn from_store(
        store_path: impl Into<PathBuf>,
        key_material: &impl KeyMaterial,
    ) -> Self {
        Self {
            key: key_material.fetch().await,
            store_path: store_path.into(),
            profiles: Arc::new(DashMap::new()),
        }
    }

    pub async fn import(&self) -> Result<()> {
        let secret = match &self.key {
            Err(e) => bail!("failed to retrieve key material: {e}"),
            Ok(v) => v,
        };

        let path = self.store_path.join(Self::FILENAME);
        let mut bytes = match fs::read(&path).await {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => bail!("failed to read profile store at {path:?}, err: {e}"),
            Ok(bytes) => bytes,
        };

        ensure!(
            bytes.len() >= 24,
            "profile store is too short ({} bytes), should be at least 24 bytes",
            bytes.len()
        );

        let contents = bytes.split_off(24);
        let nonce = bytes;

        let json = decrypt(contents, nonce, secret.expose_secret())?;
        let profiles: HashMap<String, WifiProfile> = serde_json::from_slice(&json)?;

        for (key, value) in profiles {
            self.profiles.insert(key, value);
        }

        Ok(())
    }

    pub async fn commit(&self) -> Result<()> {
        let secret = match &self.key {
            Err(e) => bail!("failed to retrieve key material: {e}"),
            Ok(v) => v,
        };

        let profiles: HashMap<_, _> = self
            .profiles
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        let json = serde_json::to_vec(&profiles)?;
        let bytes = encrypt(json, secret.expose_secret())?;

        let path = self.store_path.join(Self::FILENAME);
        fs::write(path, bytes).await?;

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

fn decrypt(bytes: Vec<u8>, mut nonce: Vec<u8>, secret: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        nonce.len() == 24,
        "none len must be 24 bytes, instead is {}",
        nonce.len()
    );

    ensure!(
        secret.len() == 32,
        "secret len must be 32 bytes, instead is {}",
        secret.len()
    );

    let nonce = XNonce::from_mut_slice(&mut nonce);

    let secret = Key::from_slice(secret);
    let cipher = XChaCha20Poly1305::new(secret);

    let plaintext = cipher
        .decrypt(nonce, bytes.as_slice())
        .map_err(|e| eyre!("failed to decrypt profiles: {e:?}"))?;

    Ok(plaintext)
}

fn encrypt(bytes: Vec<u8>, secret: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        secret.len() == 32,
        "secret len must be 32 bytes, instead is {}",
        secret.len()
    );

    let mut rng = OsRng;
    let nonce = XChaCha20Poly1305::generate_nonce(&mut rng);
    let secret = Key::from_slice(secret);

    let cipher = XChaCha20Poly1305::new(secret);
    let ciphertext = cipher
        .encrypt(&nonce, bytes.as_slice())
        .map_err(|e| eyre!("failed to encrypt profiles: {e:?}"))?;

    let mut out = Vec::new();
    out.extend_from_slice(nonce.as_slice());
    out.extend(ciphertext);

    Ok(out)
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
