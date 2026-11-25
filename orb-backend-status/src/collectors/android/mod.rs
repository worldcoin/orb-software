mod connectivity;

use super::{GlobalConnectivity, UrgentSend};
use crate::{
    backend::types::OrbStatusApiV2,
    orb_event_stream::{self, OrbEventStream, Payload},
};
use chrono::Utc;
use color_eyre::Result;
use rustix::time::{clock_gettime, ClockId};
use secrecy::SecretString;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use zenorb::Zenorb;

pub(crate) use connectivity::wifi_report;

pub struct Config {
    pub token: SecretString,
}

pub struct Collectors {
    _token_tx: watch::Sender<SecretString>,
    _connectivity_tx: watch::Sender<GlobalConnectivity>,
    state: Arc<ReporterState>,
}

#[derive(Default)]
struct ReporterState {
    wifi_ssid: Mutex<Option<String>>,
    urgent_send: UrgentSend,
}

impl Collectors {
    pub async fn new(
        config: Config,
        _shutdown_token: CancellationToken,
    ) -> Result<(
        Self,
        watch::Receiver<SecretString>,
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
                state: Arc::default(),
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
        zsession: &Zenorb,
        oes: OrbEventStream,
    ) -> Result<Vec<JoinHandle<()>>> {
        zsession
            .receiver((oes, self.state.clone()))
            .subscriber(
                orb_event_stream::KEY_EXPR,
                |(oes, state), sample| async move {
                    ingest(&oes, &state, Payload::try_from(sample)?)
                },
            )
            .run()
            .await
    }

    pub(crate) async fn snapshot(&self) -> OrbStatusApiV2 {
        let uptime = clock_gettime(ClockId::Boottime);

        OrbStatusApiV2 {
            timestamp: Utc::now(),
            uptime_sec: Some(uptime.tv_sec as f64),
            ..Default::default()
        }
    }

    pub(crate) async fn wait_for_urgent_send(&self) {
        self.state.urgent_send.wait().await;
    }

    pub(crate) fn clear_send_immediately(&self) {}
}

fn ingest(oes: &OrbEventStream, state: &ReporterState, payload: Payload) -> Result<()> {
    let report = match payload.headers.mode {
        oes::Mode::Normal => None,
        oes::Mode::Sticky | oes::Mode::CacheOnly => {
            connectivity::active_connections(&payload.event)
        }
    };
    oes.ingest(payload)?;
    if let Some(report) = report {
        connectivity::handle_connection_event(state, report)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
