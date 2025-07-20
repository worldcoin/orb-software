use orb_backend_status_dbus::{
    types::{NetStats, UpdateProgress, WifiNetwork},
    BackendStatusT,
};

#[cfg(test)]
use orb_update_agent_dbus::UpdateAgentState;
use orb_telemetry::TraceCtx;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, info_span};

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
    pub net_stats: Option<NetStats>,
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
            self.notify.notify_one();
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
            self.notify.notify_one();
        }
        Ok(())
    }

    fn provide_net_stats(
        &self,
        net_stats: NetStats,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_net_stats");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        if let Ok(mut current_status) = self.current_status.lock() {
            if let Some(current_status) = current_status.as_mut() {
                current_status.net_stats = Some(net_stats);
            } else {
                *current_status = Some(CurrentStatus {
                    net_stats: Some(net_stats),
                    ..Default::default()
                });
            }
            self.notify.notify_one();
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

    pub async fn wait_for_updates(&mut self) {
        let sleep = tokio::time::sleep(self.update_interval);
        tokio::pin!(sleep);

        tokio::select! {
            _ = self.notify.notified() => {}
            () = &mut sleep => {
                info!("sleep awake");
            }
            _ = self.shutdown_token.cancelled() => {}
        }
    }

    pub async fn send_current_status(&mut self) -> Option<CurrentStatus> {
        let current_status = self.get_available_status()?;

        let wifi_networks = current_status.wifi_networks.is_some();
        let update_progress = current_status.update_progress.is_some();
        let net_stats = current_status.net_stats.is_some();
        if !wifi_networks && !update_progress && !net_stats {
            // nothing to send
            return None;
        }

        info!(
            ?wifi_networks,
            ?update_progress,
            ?net_stats,
            "Updating backend-status"
        );

        info!("Sending status to backend: {:?}", current_status);

        match self.status_client.send_status(&current_status).await {
            Ok(_) => (),
            Err(e) => {
                error!("failed to send status: {e:?}");
            }
        };

        // don't send again until the update interval has passed
        self.last_update = Instant::now();
        Some(current_status)
    }

    fn get_available_status(&self) -> Option<CurrentStatus> {
        if let Ok(mut current_status) = self.current_status.lock() {
            if self.last_update.elapsed() >= self.update_interval {
                return current_status.take();
            }
            // too soon to send again
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::args::Args;

    use super::*;
    use orb_backend_status_dbus::types::NetIntf;
    use orb_info::{OrbId, OrbJabilId, OrbName};
    use std::{str::FromStr, time::Duration};
    use tokio::{sync::watch, time::sleep};
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
        let orb_id = OrbId::from_str("abcd1234").unwrap();
        let orb_name = OrbName::from_str("TestOrb").unwrap();
        let jabil_id = OrbJabilId::from_str("1234567890").unwrap();
        let (_, token_receiver) = watch::channel("test-orb-token".to_string());
        let shutdown_token = CancellationToken::new();
        let args = &Args {
            orb_id: Some("abcd1234".to_string()),
            orb_token: Some("test-orb-token".to_string()),
            backend: "local".to_string(),
            status_local_address: Some(mock_server.address().to_string()),
            ..Default::default()
        };

        let mut backend_status = BackendStatusImpl::new(
            StatusClient::new(
                args,
                orb_id,
                Some(orb_name),
                Some(jabil_id),
                token_receiver,
            )
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
            state: UpdateAgentState::Downloading,
        };

        // Provide update progress
        backend_status
            .provide_update_progress(test_progress.clone(), TraceCtx::collect())
            .unwrap();

        // Wait for a bit longer than the update interval
        sleep(Duration::from_millis(150)).await;

        // Get the current status and send it
        backend_status.wait_for_updates().await;
        backend_status.send_current_status().await;

        // Verify the sent status
        let sent_statuses = mock_server.received_requests().await.unwrap();
        assert_eq!(sent_statuses.len(), 1);
    }

    #[tokio::test]
    async fn test_multiple_sends() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/orbs/abcd1234/status"))
            .respond_with(ResponseTemplate::new(204))
            .expect(3)
            .mount(&mock_server)
            .await;
        let orb_id = OrbId::from_str("abcd1234").unwrap();
        let orb_name = OrbName::from_str("TestOrb").unwrap();
        let jabil_id = OrbJabilId::from_str("1234567890").unwrap();
        let (_, token_receiver) = watch::channel("test-orb-token".to_string());
        let shutdown_token = CancellationToken::new();
        let args = &Args {
            orb_id: Some("abcd1234".to_string()),
            orb_token: Some("test-orb-token".to_string()),
            backend: "local".to_string(),
            status_local_address: Some(mock_server.address().to_string()),
            ..Default::default()
        };

        let mut backend_status = BackendStatusImpl::new(
            StatusClient::new(
                args,
                orb_id,
                Some(orb_name),
                Some(jabil_id),
                token_receiver,
            )
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
                state: UpdateAgentState::Downloading,
            };

            backend_status
                .provide_update_progress(progress, TraceCtx::collect())
                .unwrap();

            // Wait for update interval
            sleep(Duration::from_millis(150)).await;

            backend_status.wait_for_updates().await;
            backend_status.send_current_status().await;
        }

        // Verify all updates were sent
        let sent_statuses = mock_server.received_requests().await.unwrap();
        assert_eq!(sent_statuses.len(), 3);
    }

    #[tokio::test]
    async fn test_net_stats_handling() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/orbs/abcd1234/status"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mock_server)
            .await;
        let orb_id = OrbId::from_str("abcd1234").unwrap();
        let orb_name = OrbName::from_str("TestOrb").unwrap();
        let jabil_id = OrbJabilId::from_str("1234567890").unwrap();
        let (_, token_receiver) = watch::channel("test-orb-token".to_string());
        let shutdown_token = CancellationToken::new();
        let args = &Args {
            orb_id: Some("abcd1234".to_string()),
            orb_token: Some("test-orb-token".to_string()),
            backend: "local".to_string(),
            status_local_address: Some(mock_server.address().to_string()),
            ..Default::default()
        };

        let mut backend_status = BackendStatusImpl::new(
            StatusClient::new(
                args,
                orb_id,
                Some(orb_name),
                Some(jabil_id),
                token_receiver,
            )
            .await
            .unwrap(),
            Duration::from_millis(100),
            shutdown_token.clone(),
        )
        .await;

        // Provide net stats
        let net_stats = NetStats {
            interfaces: vec![NetIntf {
                name: "wlan0".to_string(),
                rx_bytes: 1000,
                rx_packets: 10,
                rx_errors: 0,
                tx_bytes: 2000,
                tx_packets: 20,
                tx_errors: 0,
            }],
        };

        backend_status
            .provide_net_stats(net_stats, TraceCtx::collect())
            .unwrap();

        // Wait for update interval
        sleep(Duration::from_millis(150)).await;

        // Get the current status and send it
        backend_status.wait_for_updates().await;
        backend_status.send_current_status().await;

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
            .expect(1)
            .mount(&mock_server)
            .await;
        let orb_id = OrbId::from_str("abcd1234").unwrap();
        let orb_name = OrbName::from_str("TestOrb").unwrap();
        let jabil_id = OrbJabilId::from_str("1234567890").unwrap();
        let (_, token_receiver) = watch::channel("test-orb-token".to_string());
        let shutdown_token = CancellationToken::new();
        let args = &Args {
            orb_id: Some("abcd1234".to_string()),
            orb_token: Some("test-orb-token".to_string()),
            backend: "local".to_string(),
            status_local_address: Some(mock_server.address().to_string()),
            ..Default::default()
        };

        let mut backend_status = BackendStatusImpl::new(
            StatusClient::new(
                args,
                orb_id,
                Some(orb_name),
                Some(jabil_id),
                token_receiver,
            )
            .await
            .unwrap(),
            Duration::from_millis(100),
            shutdown_token.clone(),
        )
        .await;

        // Provide various updates
        let progress = UpdateProgress {
            download_progress: 50,
            processed_progress: 25,
            install_progress: 10,
            total_progress: 85,
            error: None,
            state: UpdateAgentState::Downloading,
        };

        backend_status
            .provide_update_progress(progress.clone(), TraceCtx::collect())
            .unwrap();

        let net_stats = NetStats {
            interfaces: vec![NetIntf {
                name: "wlan0".to_string(),
                rx_bytes: 1000,
                rx_packets: 10,
                rx_errors: 0,
                tx_bytes: 2000,
                tx_packets: 20,
                tx_errors: 0,
            }],
        };

        backend_status
            .provide_net_stats(net_stats.clone(), TraceCtx::collect())
            .unwrap();

        let wifi_networks = vec![WifiNetwork {
            ssid: "test-ssid".to_string(),
            bssid: "00:11:22:33:44:55".to_string(),
            frequency: 2412,
            signal_level: 0,
            flags: String::new(),
        }];

        backend_status
            .provide_wifi_networks(wifi_networks.clone(), TraceCtx::collect())
            .unwrap();

        // Wait for update interval
        sleep(Duration::from_millis(150)).await;

        // Get the current status and send it
        backend_status.wait_for_updates().await;
        let status = backend_status.send_current_status().await.unwrap();

        // Verify all updates were accumulated
        assert_eq!(status.update_progress, Some(progress));
        assert_eq!(status.net_stats, Some(net_stats));
        assert_eq!(status.wifi_networks, Some(wifi_networks));
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
        let orb_id = OrbId::from_str("abcd1234").unwrap();
        let orb_name = OrbName::from_str("TestOrb").unwrap();
        let jabil_id = OrbJabilId::from_str("1234567890").unwrap();
        let (_, token_receiver) = watch::channel("test-orb-token".to_string());
        let shutdown_token = CancellationToken::new();
        let args = &Args {
            orb_id: Some("abcd1234".to_string()),
            orb_token: Some("test-orb-token".to_string()),
            backend: "local".to_string(),
            status_local_address: Some(mock_server.address().to_string()),
            ..Default::default()
        };

        let mut backend_status = BackendStatusImpl::new(
            StatusClient::new(
                args,
                orb_id,
                Some(orb_name),
                Some(jabil_id),
                token_receiver,
            )
            .await
            .unwrap(),
            Duration::from_millis(100),
            shutdown_token.clone(),
        )
        .await;

        // Trigger shutdown
        shutdown_token.cancel();

        // Verify that wait_for_updates returns None after shutdown
        backend_status.wait_for_updates().await;
    }
}
