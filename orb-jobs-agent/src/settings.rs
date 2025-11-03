use crate::args::Args;
use color_eyre::{
    eyre::{eyre, Context, ContextCompat},
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
    /// Path to the calibration file (configurable for testing)
    pub calibration_file_path: PathBuf,
    /// Path to the OS release file (configurable for testing)
    pub os_release_path: PathBuf,
    /// Path to the versions file (configurable for testing)
    pub versions_file_path: PathBuf,
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
                let shutdown_token = CancellationToken::new();
                let get_token = async || {
                    let connection = Connection::session()
                        .await
                        .map_err(|e| eyre!("failed to establish zbus conn: {e}"))?;

                    TokenTaskHandle::spawn(&connection, &shutdown_token)
                        .await
                        .wrap_err("failed to get auth token!")
                };

                let token_rec_fut = async {
                    loop {
                        match get_token().await {
                            Err(e) => {
                                warn!("{e}! trying again in 5s");
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
            calibration_file_path: PathBuf::from("/usr/persistent/calibration.json"),
            os_release_path: PathBuf::from("/etc/os-release"),
            versions_file_path: PathBuf::from("/usr/persistent/versions.json"),
        })
    }
}
