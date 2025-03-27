use crate::{args::Args, backend::status::StatusClient};
use color_eyre::eyre::{Result, WrapErr};
use orb_backend_status_dbus::{BackendStatus, BackendStatusIface, WifiNetwork};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::error;
use zbus::ConnectionBuilder;

#[derive(Debug, Clone)]
pub struct BackendStatusImpl {
    last_update: Instant,
    current_status: Arc<Mutex<Option<CurrentStatus>>>,
    notify: Arc<Notify>,
    shutdown_token: CancellationToken,
    status_client: StatusClient,
}

#[derive(Debug, Default)]
pub struct CurrentStatus {
    pub wifi_networks: Option<Vec<WifiNetwork>>,
    pub update_progress: Option<UpdateProgress>,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct UpdateProgress {
    pub download_progress: u64,
    pub install_progress: u64,
    pub fetched_progress: u64,
    pub processed_progress: u64,
    pub total_progress: u64,
    pub errors: Option<String>,
}

impl BackendStatusIface for BackendStatusImpl {
    fn provide_wifi_networks(&mut self, wifi_networks: Vec<WifiNetwork>) {
        if let Ok(mut current_status) = self.current_status.lock() {
            if let Some(current_status) = current_status.as_mut() {
                current_status.wifi_networks = Some(wifi_networks);
            } else {
                *current_status = Some(CurrentStatus {
                    wifi_networks: Some(wifi_networks),
                    ..Default::default()
                });
            }
            if self.last_update.elapsed() > Duration::from_secs(10) {
                self.notify.notify_one();
            }
        }
    }
}

impl BackendStatusImpl {
    pub async fn new(
        status_client: StatusClient,
        shutdown_token: CancellationToken,
    ) -> Self {
        Self {
            last_update: Instant::now(),
            notify: Arc::new(Notify::new()),
            shutdown_token,
            status_client,
            current_status: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn wait_for_updates(
        &mut self,
        timeout: Duration,
    ) -> Option<CurrentStatus> {
        loop {
            tokio::select! {
                _ = self.notify.notified() => {
                    if let Ok(mut current_status) = self.current_status.lock() {
                        return current_status.take();
                    }
                }
                _ = tokio::time::sleep(timeout) => {
                    if let Ok(mut current_status) = self.current_status.lock() {
                        return current_status.take();
                    }
                }
                _ = self.shutdown_token.cancelled() => {
                    return None;
                }
            }
        }
    }

    pub fn provide_update_progress(&mut self, update_progress: UpdateProgress) {
        if let Ok(mut current_status) = self.current_status.lock() {
            if let Some(current_status) = current_status.as_mut() {
                current_status.update_progress = Some(update_progress);
            } else {
                *current_status = Some(CurrentStatus {
                    update_progress: Some(update_progress),
                    ..Default::default()
                });
            }
            if self.last_update.elapsed() > Duration::from_secs(10) {
                self.notify.notify_one();
            }
        }
    }

    pub async fn send_current_status(&mut self, current_status: &CurrentStatus) {
        match self.status_client.send_status(current_status).await {
            Ok(_) => (),
            Err(e) => {
                error!("failed to send status: {e:?}");
            }
        };
        self.last_update = Instant::now();
    }
}

pub async fn setup_dbus(
    args: &Args,
    shutdown_token: CancellationToken,
) -> Result<(BackendStatusImpl, zbus::Connection)> {
    let backend_status_impl = BackendStatusImpl::new(
        StatusClient::new(args, shutdown_token.clone()).await?,
        shutdown_token.clone(),
    )
    .await;

    let dbus_conn = ConnectionBuilder::session()
        .wrap_err("failed creating a new session dbus connection")?
        .name("org.worldcoin.BackendStatus")
        .wrap_err(
            "failed to register dbus connection name: `org.worldcoin.BackendStatus1``",
        )?
        .serve_at(
            "/org/worldcoin/BackendStatus1",
            BackendStatus::from(backend_status_impl.clone()),
        )
        .wrap_err("failed to serve dbus interface at `/org/worldcoin/BackendStatus1`")?
        .build()
        .await;

    let dbus_conn = match dbus_conn {
        Ok(conn) => conn,
        Err(e) => {
            error!("failed to setup dbus connection: {e:?}");
            return Err(e.into());
        }
    };

    Ok((backend_status_impl, dbus_conn))
}
