use super::GlobalConnectivity;
use crate::{backend::types::OrbStatusApiV2, orb_event_stream::OrbEventStream};
use chrono::Utc;
use color_eyre::Result;
use std::{future::pending, path::PathBuf};
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use zenorb::Zenorb;

pub struct Config {
    pub token: String,
}

pub struct Collectors {
    _token_tx: watch::Sender<String>,
    _connectivity_tx: watch::Sender<GlobalConnectivity>,
}

impl Collectors {
    pub async fn new(
        config: Config,
        _shutdown_token: CancellationToken,
    ) -> Result<(
        Self,
        watch::Receiver<String>,
        watch::Receiver<GlobalConnectivity>,
    )> {
        let (token_tx, token_receiver) = watch::channel(config.token);
        // Android has no connd collector yet. Allow HTTP attempts; this is not
        // a connectivity probe or a claim that the backend is reachable.
        let (connectivity_tx, connectivity_receiver) =
            watch::channel(GlobalConnectivity::Connected { ssid: None });

        Ok((
            Self {
                _token_tx: token_tx,
                _connectivity_tx: connectivity_tx,
            },
            token_receiver,
            connectivity_receiver,
        ))
    }

    pub(crate) fn spawn_reporters(
        &self,
        _procfs: PathBuf,
        _shutdown_token: CancellationToken,
    ) -> Vec<JoinHandle<()>> {
        Vec::new()
    }

    pub(crate) async fn subscribe(
        &self,
        _zsession: &Zenorb,
        _oes: OrbEventStream,
    ) -> Result<Vec<JoinHandle<()>>> {
        Ok(Vec::new())
    }

    pub(crate) async fn snapshot(&self) -> OrbStatusApiV2 {
        OrbStatusApiV2 {
            timestamp: Utc::now(),
            ..Default::default()
        }
    }

    pub(crate) async fn wait_for_urgent_send(&self) {
        pending::<()>().await;
    }

    pub(crate) fn clear_send_immediately(&self) {}
}

#[cfg(test)]
mod tests;
