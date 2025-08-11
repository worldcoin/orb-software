use crate::args::Args;
use color_eyre::{
    eyre::{Context, ContextCompat},
    Result,
};
use orb_endpoints::{v1::Endpoints, Backend};
use orb_info::{OrbId, TokenTaskHandle};
use orb_relay_client::Auth;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use zbus::Connection;

#[derive(Debug, Clone)]
pub struct Settings {
    pub orb_id: OrbId,
    pub auth: Auth,
    pub relay_host: String,
    pub relay_namespace: String,
    pub target_service_id: String,
    /// Filesystem path used to persist data
    pub store_path: PathBuf,
}

impl Settings {
    pub async fn from_args(args: &Args, store_path: impl AsRef<Path>) -> Result<Self> {
        let orb_id = if let Some(id) = &args.orb_id {
            OrbId::from_str(id)?
        } else {
            OrbId::read().await?
        };

        let relay_host = args
            .relay_host
            .clone()
            .or_else(|| {
                Backend::from_env()
                    .ok()
                    .map(|backend| Endpoints::new(backend, &orb_id).relay.to_string())
            })
            .wrap_err("could not get Backend Endpoint from env")?;

        // Get token from DBus
        let auth = match &args.orb_token {
            Some(t) => Auth::Token(t.as_str().into()),
            None => {
                // shutdown token is never used, so no need to propagate it elsewhere.
                // when program ends task will be dropped.
                let shutdown_token = CancellationToken::new();
                let connection = Connection::session().await?;
                let token_rec_fut = async {
                    loop {
                        match TokenTaskHandle::spawn(&connection, &shutdown_token).await
                        {
                            Err(e) => {
                                warn!("failed to get auth token! trying again in 5s. err: {e}");
                                time::sleep(Duration::from_secs(5)).await;
                                continue;
                            }

                            Ok(t) => break t.token_recv,
                        }
                    }
                };

                let token_rec = time::timeout(Duration::from_secs(60), token_rec_fut)
                    .await
                    .wrap_err("could not get auth token after 60s")?;

                Auth::TokenReceiver(token_rec)
            }
        };

        let relay_namespace = args
            .relay_namespace
            .clone()
            .wrap_err("relay namespace MUST be provided")?;

        let target_service_id = args
            .target_service_id
            .clone()
            .wrap_err("target service id MUST be provided")?;

        Ok(Self {
            orb_id,
            auth,
            relay_host,
            relay_namespace,
            target_service_id,
            store_path: store_path.as_ref().to_path_buf(),
        })
    }
}
