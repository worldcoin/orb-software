//! Persists monitoring-token state in OP-TEE secure storage and translates
//! between stored bytes, a current token, and the explicit empty state.

use crate::Token;
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use orb_secure_storage_ca::{optee::OpteeBackend, StorageDomain};
use std::sync::{Arc, Mutex};
use tokio::task;
use tracing::warn;

const SS_TOKEN_KEY: &str = "monitoring-token";

#[cfg_attr(feature = "testing", faux::create)]
#[derive(Clone)]
pub struct SecureStorage(Arc<Mutex<orb_secure_storage_ca::Client<OpteeBackend>>>);

#[cfg_attr(feature = "testing", faux::methods)]
impl SecureStorage {
    pub async fn new() -> Result<Self> {
        let client = task::spawn_blocking(|| {
            let mut ctx =
                orb_secure_storage_ca::reexported_crates::optee_teec::Context::new()
                    .wrap_err("failed to initialize optee context")?;

            orb_secure_storage_ca::Client::new(&mut ctx, StorageDomain::WifiProfiles)
        })
        .await??;

        Ok(Self(Arc::new(Mutex::new(client))))
    }

    pub async fn put(&self, token: &Token) -> Result<()> {
        let ss = self.0.clone();
        let bytes = serde_json::to_vec(&token)?;
        task::spawn_blocking(move || -> Result<()> {
            ss.lock()
                .map_err(|e| eyre!("{e:?}"))?
                .put(SS_TOKEN_KEY, &bytes)?;

            Ok(())
        })
        .await??;

        Ok(())
    }

    /// Replaces the persisted monitoring token with the empty token state.
    ///
    /// Returns an error when serialization, secure-storage access, locking,
    /// or the blocking task fails.
    pub async fn clear(&self) -> Result<()> {
        let ss = self.0.clone();
        let bytes = serde_json::to_vec(&Option::<Token>::None)?;
        task::spawn_blocking(move || -> Result<()> {
            ss.lock()
                .map_err(|e| eyre!("{e:?}"))?
                .put(SS_TOKEN_KEY, &bytes)?;

            Ok(())
        })
        .await??;

        Ok(())
    }

    pub async fn get(&self) -> Result<Option<Token>> {
        let ss = self.0.clone();
        let bytes = task::spawn_blocking(move || -> Result<_> {
            let bytes = ss.lock().map_err(|e| eyre!("{e:?}"))?.get(SS_TOKEN_KEY)?;
            Ok(bytes)
        })
        .await??;

        let token = bytes.and_then(|b| {
            serde_json::from_slice::<Option<Token>>(&b)
                .inspect_err(|e| {
                    warn!("failed to deserialize Token from secure storage {e:?}")
                })
                .ok()
                .flatten()
        });

        Ok(token)
    }
}
