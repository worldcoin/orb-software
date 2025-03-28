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

use crate::backend::status::BackendStatusClientT;

#[derive(Clone)]
pub struct BackendStatusImpl {
    status_client: Arc<Box<dyn BackendStatusClientT>>,
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

#[derive(Debug, Default, Eq, PartialEq, Clone)]
pub struct UpdateProgress {
    pub download_progress: u64,
    pub processed_progress: u64,
    pub install_progress: u64,
    pub total_progress: u64,
    pub errors: Option<Vec<String>>,
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
            if self.last_update.elapsed() > self.update_interval {
                self.notify.notify_one();
            }
        }
    }
}

impl BackendStatusImpl {
    pub async fn new(
        status_client: Arc<Box<dyn BackendStatusClientT>>,
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
            if self.last_update.elapsed() > self.update_interval {
                self.notify.notify_one();
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
    backend_status_impl: impl BackendStatusIface,
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
    use crate::backend::status::BackendStatusClientT;

    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    #[derive(Debug, Clone)]
    struct MockStatusClient {
        sent_statuses: Arc<Mutex<Vec<CurrentStatus>>>,
    }

    impl MockStatusClient {
        fn new() -> Self {
            Self {
                sent_statuses: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn get_sent_statuses(&self) -> Vec<CurrentStatus> {
            self.sent_statuses.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl BackendStatusClientT for MockStatusClient {
        async fn send_status(&self, current_status: &CurrentStatus) -> Result<String> {
            self.sent_statuses
                .lock()
                .unwrap()
                .push(current_status.clone());
            Ok("success".to_string())
        }
    }

    #[tokio::test]
    async fn test_update_progress_handling() {
        let mock_client = MockStatusClient::new();
        let update_interval = Duration::from_millis(100);
        let shutdown_token = CancellationToken::new();

        let mut backend_status = BackendStatusImpl::new(
            Arc::new(Box::new(mock_client.clone())),
            update_interval,
            shutdown_token.clone(),
        )
        .await;

        // Create test update progress
        let test_progress = UpdateProgress {
            download_progress: 50,
            processed_progress: 25,
            install_progress: 10,
            total_progress: 85,
            errors: None,
        };

        // Provide update progress
        backend_status.provide_update_progress(test_progress.clone());

        // Wait for a bit longer than the update interval
        sleep(Duration::from_millis(150)).await;

        // Get the current status and send it
        if let Some(status) = backend_status.wait_for_updates().await {
            backend_status.send_current_status(&status).await;
        }

        // Verify the sent status
        let sent_statuses = mock_client.get_sent_statuses();
        assert_eq!(sent_statuses.len(), 1);

        let sent_status = &sent_statuses[0];
        assert!(sent_status.wifi_networks.is_none());
        assert!(sent_status.update_progress.is_some());

        let sent_progress = sent_status.update_progress.as_ref().unwrap();
        assert_eq!(sent_progress.download_progress, 50);
        assert_eq!(sent_progress.processed_progress, 25);
        assert_eq!(sent_progress.install_progress, 10);
        assert_eq!(sent_progress.total_progress, 85);
        assert!(sent_progress.errors.is_none());
    }

    #[tokio::test]
    async fn test_multiple_updates() {
        let mock_client = MockStatusClient::new();
        let update_interval = Duration::from_millis(100);
        let shutdown_token = CancellationToken::new();

        let mut backend_status = BackendStatusImpl::new(
            Arc::new(Box::new(mock_client.clone())),
            update_interval,
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
                errors: None,
            };

            backend_status.provide_update_progress(progress);

            // Wait for update interval
            sleep(Duration::from_millis(150)).await;

            if let Some(status) = backend_status.wait_for_updates().await {
                backend_status.send_current_status(&status).await;
            }
        }

        // Verify all updates were sent
        let sent_statuses = mock_client.get_sent_statuses();
        assert_eq!(sent_statuses.len(), 3);

        for (i, status) in sent_statuses.iter().enumerate() {
            let progress = status.update_progress.as_ref().unwrap();
            assert_eq!(progress.download_progress, (i as u64) * 30);
            assert_eq!(progress.processed_progress, (i as u64) * 20);
            assert_eq!(progress.install_progress, (i as u64) * 10);
            assert_eq!(progress.total_progress, (i as u64) * 60);
        }
    }

    #[tokio::test]
    async fn test_shutdown() {
        let mock_client = Arc::new(
            Box::new(MockStatusClient::new()) as Box<dyn BackendStatusClientT>
        );
        let update_interval = Duration::from_millis(100);
        let shutdown_token = CancellationToken::new();

        let mut backend_status = BackendStatusImpl::new(
            mock_client.clone(),
            update_interval,
            shutdown_token.clone(),
        )
        .await;

        // Trigger shutdown
        shutdown_token.cancel();

        // Verify that wait_for_updates returns None after shutdown
        assert!(backend_status.wait_for_updates().await.is_none());
    }
}
