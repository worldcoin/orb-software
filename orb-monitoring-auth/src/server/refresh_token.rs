//! Refreshes monitoring credentials from the backend.
//!
//! The task validates decoded tokens before persisting or publishing them; an
//! invalid response leaves the current token unchanged and enters supervisor
//! retry handling.

use crate::{
    server::{Clock, SecureStorage, StoredToken},
    Token,
};
use color_eyre::{
    eyre::{bail, eyre, Context},
    Result,
};
use orb_info::{OrbId, TokenTaskHandle};
use speare::mini::{self};
use std::time::Duration;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub struct Args {
    pub token: StoredToken,
    pub secure_storage: SecureStorage,
    pub endpoint: String,
    pub dbus: zbus::Connection,
    pub interval: Duration,
    pub clock: Clock,
    pub orb_id: OrbId,
}

pub async fn task(ctx: mini::Ctx<Args>) -> Result<()> {
    let refresher = async || -> Result<()> {
        let token = {
            let t = ctx
                .token
                .read()
                .map_err(|e| eyre!("token RwLock poisoned: {e:?}"))?;

            (*t).clone()
        };

        if let Some(token) = token
            && !token.needs_refresh_at(ctx.clock.now())
        {
            return Ok(());
        }

        let ct = CancellationToken::new();
        let attest_token = TokenTaskHandle::spawn(&ctx.dbus, &ct).await?;
        let attest_token = attest_token.token_recv.borrow().to_owned();
        ct.cancel();

        let reqwest = orb_security_utils::reqwest::client_builder().build()?;
        let res = reqwest
            .get(&ctx.endpoint)
            .basic_auth(&ctx.orb_id, Some(attest_token))
            .timeout(Duration::from_secs(10))
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let err = res.text().await.unwrap_or_default();
            bail!("failed fetching monitoring token: {status}, error: '{err}'",);
        }

        let new_token = res
            .bytes()
            .await
            .wrap_err("failed to deserialize token json")?;

        let new_token: Token = serde_json::from_slice(&new_token)?;

        if new_token.needs_refresh_at(ctx.clock.now()) {
            bail!("backend returned a monitoring token that already needs refresh");
        }

        ctx.secure_storage.put(&new_token).await?;

        let mut old_token = ctx
            .token
            .write()
            .map_err(|e| eyre!("poisoned token rwlock {e}"))?;

        *old_token = Some(new_token);

        Ok(())
    };

    loop {
        refresher()
            .await
            .inspect_err(|e| warn!("failed to refresh monitoring token: {e:?}"))?;

        time::sleep(ctx.interval).await;
    }
}
