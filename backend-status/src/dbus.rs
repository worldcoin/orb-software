use color_eyre::eyre::{Result, WrapErr};
use orb_backend_status_dbus::{
    types::{UpdateProgress, WifiNetwork},
    BackendStatus, BackendStatusT,
};
use orb_telemetry::TraceCtx;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{error, info_span};
use zbus::ConnectionBuilder;

use crate::backend::status::{BackendStatusClientT, StatusClient};

#[derive(Debug, Clone)]
pub struct BackendStatusImpl {
    status_client: StatusClient,
    current_status: Arc<Mutex<Option<CurrentStatus>>>,
    notify: Arc<Notify>,
    last_update: Instant,
    update_interval: Duration,
    shutdown_token: CancellationToken,
}

#[derive(Debug, Default, Clone)]
pub struct CurrentStatus {
    pub wifi_networks: Option<Vec<WifiNetwork>>,
    pub update_progress: Option<UpdateProgress>,
}

impl BackendStatusT for BackendStatusImpl {
    fn provide_wifi_networks(
        &self,
        wifi_networks: Vec<WifiNetwork>,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_wifi_networks");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        if let Ok(mut current_status) = self.current_status.lock() {
            if let Some(current_status) = current_status.as_mut() {
                current_status.wifi_networks = Some(wifi_networks);
            } else {
                *current_status = Some(CurrentStatus {
                    wifi_networks: Some(wifi_networks),
                    ..Default::default()
                });
            }
            if self.last_update.elapsed() > self.update_interval {
                self.notify.notify_one();
            }
        }
        Ok(())
    }

    fn provide_update_progress(
        &self,
        update_progress: UpdateProgress,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_update_progress");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        if let Ok(mut current_status) = self.current_status.lock() {
            if let Some(current_status) = current_status.as_mut() {
                current_status.update_progress = Some(update_progress);
            } else {
                *current_status = Some(CurrentStatus {
                    update_progress: Some(update_progress),
                    ..Default::default()
                });
            }
            if self.last_update.elapsed() > self.update_interval {
                self.notify.notify_one();
            }
        }
        Ok(())
    }
}

impl BackendStatusImpl {
    pub async fn new(
        status_client: StatusClient,
        update_interval: Duration,
        shutdown_token: CancellationToken,
    ) -> Self {
        Self {
            status_client,
            current_status: Arc::new(Mutex::new(None)),
            notify: Arc::new(Notify::new()),
            last_update: Instant::now(),
            update_interval,
            shutdown_token,
        }
    }

    pub async fn wait_for_updates(&mut self) -> Option<CurrentStatus> {
        loop {
            tokio::select! {
                _ = self.notify.notified() => {
                        if let Ok(mut current_status) = self.current_status.lock() {
                            return current_status.take();
                    }
                }
                _ = tokio::time::sleep(self.update_interval) => {
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

    pub async fn send_current_status(&mut self, current_status: &CurrentStatus) {
        // set last update before and after sending status to ensure we don't send the same status twice
        self.last_update = Instant::now();
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
    backend_status_impl: impl BackendStatusT,
) -> Result<zbus::Connection> {
    let dbus_conn = ConnectionBuilder::session()
        .wrap_err("failed creating a new session dbus connection")?
        .name("org.worldcoin.BackendStatus1")
        .wrap_err(
            "failed to register dbus connection name: `org.worldcoin.BackendStatus1``",
        )?
        .serve_at(
            "/org/worldcoin/BackendStatus1",
            BackendStatus::from(backend_status_impl),
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

    Ok(dbus_conn)
}

#[cfg(test)]
mod tests {
    use crate::args::Args;

    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn test_update_progress_handling() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/orbs/abcd1234/status"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mock_server)
            .await;
        let shutdown_token = CancellationToken::new();
        let args = &Args {
            orb_id: Some("abcd1234".to_string()),
            orb_token: Some("test-orb-token".to_string()),
            backend: "local".to_string(),
            status_local_address: mock_server.address().to_string(),
            ..Default::default()
        };

        let mut backend_status = BackendStatusImpl::new(
            StatusClient::new(args, shutdown_token.clone())
                .await
                .unwrap(),
            Duration::from_millis(100),
            shutdown_token.clone(),
        )
        .await;

        // Create test update progress
        let test_progress = UpdateProgress {
            download_progress: 50,
            processed_progress: 25,
            install_progress: 10,
            total_progress: 85,
            error: None,
        };

        // Provide update progress
        backend_status
            .provide_update_progress(test_progress.clone(), TraceCtx::collect())
            .unwrap();

        // Wait for a bit longer than the update interval
        sleep(Duration::from_millis(150)).await;

        // Get the current status and send it
        if let Some(status) = backend_status.wait_for_updates().await {
            backend_status.send_current_status(&status).await;
        }

        // Verify the sent status
        let sent_statuses = mock_server.received_requests().await.unwrap();
        assert_eq!(sent_statuses.len(), 1);
    }

    #[tokio::test]
    async fn test_multiple_updates() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/orbs/abcd1234/status"))
            .respond_with(ResponseTemplate::new(204))
            .expect(3)
            .mount(&mock_server)
            .await;
        let shutdown_token = CancellationToken::new();
        let args = &Args {
            orb_id: Some("abcd1234".to_string()),
            orb_token: Some("test-orb-token".to_string()),
            backend: "local".to_string(),
            status_local_address: mock_server.address().to_string(),
            ..Default::default()
        };

        let mut backend_status = BackendStatusImpl::new(
            StatusClient::new(args, shutdown_token.clone())
                .await
                .unwrap(),
            Duration::from_millis(100),
            shutdown_token.clone(),
        )
        .await;

        // Send multiple updates
        for i in 0..3 {
            let progress = UpdateProgress {
                download_progress: i * 30,
                processed_progress: i * 20,
                install_progress: i * 10,
                total_progress: i * 60,
                error: None,
            };

            backend_status
                .provide_update_progress(progress, TraceCtx::collect())
                .unwrap();

            // Wait for update interval
            sleep(Duration::from_millis(150)).await;

            if let Some(status) = backend_status.wait_for_updates().await {
                backend_status.send_current_status(&status).await;
            }
        }

        // Verify all updates were sent
        let sent_statuses = mock_server.received_requests().await.unwrap();
        assert_eq!(sent_statuses.len(), 3);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/orbs/abcd1234/status"))
            .respond_with(ResponseTemplate::new(204))
            .expect(0)
            .mount(&mock_server)
            .await;
        let shutdown_token = CancellationToken::new();
        let args = &Args {
            orb_id: Some("abcd1234".to_string()),
            orb_token: Some("test-orb-token".to_string()),
            backend: "local".to_string(),
            status_local_address: mock_server.address().to_string(),
            ..Default::default()
        };

        let mut backend_status = BackendStatusImpl::new(
            StatusClient::new(args, shutdown_token.clone())
                .await
                .unwrap(),
            Duration::from_millis(100),
            shutdown_token.clone(),
        )
        .await;

        // Trigger shutdown
        shutdown_token.cancel();

        // Verify that wait_for_updates returns None after shutdown
        assert!(backend_status.wait_for_updates().await.is_none());
    }
}
