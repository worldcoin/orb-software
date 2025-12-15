use crate::backend::status::StatusClient;
use color_eyre::eyre::Result as EyreResult;
use orb_backend_status_dbus::{
    types::{
        CellularStatus, ConndReport, CoreStats, NetStats, SignupState, UpdateProgress,
        WifiNetwork,
    },
    BackendStatusT,
};

use orb_telemetry::TraceCtx;
use orb_update_agent_dbus::UpdateAgentState;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, info_span};

#[derive(Clone)]
enum Sender {
    NotReady,
    Real(StatusClient),
    #[cfg(test)]
    Mock(MockSender),
}

#[cfg(test)]
#[derive(Clone, Default)]
struct MockSender {
    sends: Arc<Mutex<usize>>,
}

#[cfg(test)]
impl MockSender {
    fn send_count(&self) -> usize {
        self.sends.lock().map(|x| *x).unwrap_or(0)
    }

    async fn send_status(&self, _current_status: &CurrentStatus) -> EyreResult<()> {
        if let Ok(mut sends) = self.sends.lock() {
            *sends += 1;
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct BackendStatusImpl {
    sender: Arc<Mutex<Sender>>,
    current_status: Arc<Mutex<CurrentStatus>>,
    changed: Arc<Mutex<bool>>,
    notify: Arc<Notify>,
    last_update: Instant,
    update_interval: Duration,
    shutdown_token: CancellationToken,
    send_immediately: Arc<Mutex<bool>>,
}

#[derive(Debug, Default, Clone)]
pub struct CurrentStatus {
    pub wifi_networks: Option<Vec<WifiNetwork>>,
    pub update_progress: Option<UpdateProgress>,
    pub net_stats: Option<NetStats>,
    pub cellular_status: Option<CellularStatus>,
    pub core_stats: Option<CoreStats>,
    pub signup_state: Option<SignupState>,
    pub connd_report: Option<ConndReport>,
}

impl BackendStatusT for BackendStatusImpl {
    fn provide_update_progress(
        &self,
        update_progress: UpdateProgress,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_update_progress");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        let Ok(mut current_status) = self.current_status.lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.update_progress = Some(update_progress);
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }
        self.notify.notify_one();

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

        let Ok(mut current_status) = self.current_status.lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.net_stats = Some(net_stats);
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }
        self.notify.notify_one();

        Ok(())
    }

    fn provide_cellular_status(&self, status: CellularStatus) -> zbus::fdo::Result<()> {
        let Ok(mut current_status) = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.cellular_status = Some(status);
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }
        self.notify.notify_one();

        Ok(())
    }

    fn provide_core_stats(
        &self,
        core_stats: CoreStats,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_core_stats");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        let Ok(mut current_status) = self.current_status.lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.core_stats = Some(core_stats);
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }
        self.notify.notify_one();

        Ok(())
    }

    fn provide_signup_state(
        &self,
        signup_state: SignupState,
        trace_ctx: TraceCtx,
    ) -> zbus::fdo::Result<()> {
        let span = info_span!("backend-status::provide_signup_state");
        trace_ctx.apply(&span);
        let _guard = span.enter();

        let Ok(mut current_status) = self.current_status.lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.signup_state = Some(signup_state);
        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }

        Ok(())
    }

    fn provide_connd_report(
        &self,
        report: orb_backend_status_dbus::types::ConndReport,
    ) -> zbus::fdo::Result<()> {
        let Ok(mut current_status) = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
        else {
            return Ok(());
        };

        current_status.wifi_networks = Some(report.scanned_networks.clone());
        current_status.connd_report = Some(report);

        if let Ok(mut changed) = self.changed.lock() {
            *changed = true;
        }

        if let Ok(mut send_immediately) = self.send_immediately.lock() {
            *send_immediately = true;
        }

        self.notify.notify_one();

        Ok(())
    }
}

impl BackendStatusImpl {
    pub fn new(update_interval: Duration, shutdown_token: CancellationToken) -> Self {
        Self {
            sender: Arc::new(Mutex::new(Sender::NotReady)),
            current_status: Arc::new(Mutex::new(CurrentStatus::default())),
            changed: Arc::new(Mutex::new(false)),
            notify: Arc::new(Notify::new()),
            last_update: Instant::now(),
            update_interval,
            shutdown_token,
            send_immediately: Arc::new(Mutex::new(false)),
        }
    }

    pub fn set_status_client(&self, status_client: StatusClient) {
        let Ok(mut guard) = self
            .sender
            .lock()
            .inspect_err(|e| error!("failed to acquire sender lock: {e}"))
        else {
            return;
        };

        *guard = Sender::Real(status_client);
    }

    #[cfg(test)]
    fn set_mock_sender(&self, mock: MockSender) {
        let Ok(mut guard) = self
            .sender
            .lock()
            .inspect_err(|e| error!("failed to acquire sender lock: {e}"))
        else {
            return;
        };

        *guard = Sender::Mock(mock);
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
        let current_status = self
            .current_status
            .lock()
            .inspect_err(|e| error!("failed to acquire current status lock: {e}"))
            .ok()?
            .clone();

        let changed = self
            .changed
            .lock()
            .inspect_err(|e| error!("failed to acquire changed lock: {e}"))
            .ok()
            .map(|v| *v)
            .unwrap_or(false);

        let wifi_networks = current_status.wifi_networks.is_some();
        let update_progress = current_status.update_progress.is_some();
        let net_stats = current_status.net_stats.is_some();
        let cellular_status = current_status.cellular_status.is_some();

        let core_stats = current_status.core_stats.is_some();
        if !wifi_networks
            && !update_progress
            && !net_stats
            && !cellular_status
            && !core_stats
        {
            // nothing to send
            return None;
        }

        let has_reboot_state = current_status
            .update_progress
            .as_ref()
            .map(|progress| progress.state == UpdateAgentState::Rebooting)
            .unwrap_or(false);

        let should_send_immediately = self
            .send_immediately
            .lock()
            .map(|flag| *flag)
            .unwrap_or(false);

        // If nothing changed since the last successful send, don't re-send the same snapshot.
        if !changed && !has_reboot_state && !should_send_immediately {
            return None;
        }

        if !has_reboot_state
            && !should_send_immediately
            && self.last_update.elapsed() < self.update_interval
        {
            // too soon to send again
            return None;
        }

        info!(
            ?wifi_networks,
            ?update_progress,
            ?net_stats,
            ?cellular_status,
            ?core_stats,
            "Updating backend-status"
        );

        let sender = self
            .sender
            .lock()
            .inspect_err(|e| error!("failed to acquire sender lock: {e}"))
            .ok()
            .map(|guard| guard.clone())
            .unwrap_or(Sender::NotReady);

        let send_result = match sender {
            Sender::NotReady => {
                info!("status client not ready yet (missing auth token?) - skipping send");
                return None;
            }
            Sender::Real(status_client) => status_client.send_status(&current_status).await,
            #[cfg(test)]
            Sender::Mock(mock) => mock.send_status(&current_status).await,
        };

        match send_result {
            Ok(_) => {
                // don't send again until the update interval has passed
                self.last_update = Instant::now();

                if let Ok(mut changed) = self.changed.lock() {
                    *changed = false;
                }

                if let Ok(mut send_immediately) = self.send_immediately.lock() {
                    *send_immediately = false;
                }
            }
            Err(e) => {
                error!("failed to send status: {e:?}");
            }
        };

        Some(current_status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orb_backend_status_dbus::types::{
        Battery, Location, NetIntf, OrbVersion, Ssd, Temperature, WifiNetwork,
    };
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_update_progress_handling() {
        let shutdown_token = CancellationToken::new();
        let mut backend_status =
            BackendStatusImpl::new(Duration::from_millis(100), shutdown_token.clone());
        let mock = MockSender::default();
        backend_status.set_mock_sender(mock.clone());

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

        assert_eq!(mock.send_count(), 1);
    }

    #[tokio::test]
    async fn test_update_core_stats() {
        let shutdown_token = CancellationToken::new();
        let mut backend_status =
            BackendStatusImpl::new(Duration::from_millis(100), shutdown_token.clone());
        let mock = MockSender::default();
        backend_status.set_mock_sender(mock.clone());

        // Provide core stats
        let core_stats = CoreStats {
            battery: Battery {
                level: 0.5,
                is_charging: true,
            },
            wifi: Some(WifiNetwork {
                ssid: "test-ssid".to_string(),
                bssid: "00:11:22:33:44:55".to_string(),
                frequency: 2412,
                signal_level: 100,
            }),
            temperature: Temperature {
                cpu: 0.5,
                gpu: 0.5,
                front_unit: 0.5,
                front_pcb: 0.5,
                backup_battery: 0.5,
                battery_pcb: 0.5,
                battery_cell: 0.5,
                liquid_lens: 0.5,
                main_accelerometer: 0.5,
                main_mcu: 0.5,
                mainboard: 0.5,
                security_accelerometer: 0.5,
                security_mcu: 0.5,
                battery_pack: 0.5,
                ssd: 0.5,
                wifi: 0.5,
                main_board_usb_hub_bot: 0.5,
                main_board_usb_hub_top: 0.5,
                main_board_security_supply: 0.5,
                main_board_audio_amplifier: 0.5,
                power_board_super_cap_charger: 0.5,
                power_board_pvcc_supply: 0.5,
                power_board_super_caps_left: 0.5,
                power_board_super_caps_right: 0.5,
                front_unit_850_730_left_top: 0.5,
                front_unit_850_730_left_bottom: 0.5,
                front_unit_850_730_right_top: 0.5,
                front_unit_850_730_right_bottom: 0.5,
                front_unit_940_left_top: 0.5,
                front_unit_940_left_bottom: 0.5,
                front_unit_940_right_top: 0.5,
                front_unit_940_right_bottom: 0.5,
                front_unit_940_center_top: 0.5,
                front_unit_940_center_bottom: 0.5,
                front_unit_white_top: 0.5,
                front_unit_shroud_rgb_top: 0.5,
            },
            location: Location {
                latitude: 0.5,
                longitude: 0.5,
            },
            ssd: Ssd {
                file_left: 100,
                space_left: 100,
                signup_left_to_upload: 100,
            },
            version: OrbVersion {
                current_release: "1.0.0".to_string(),
            },
            mac_address: "00:11:22:33:44:55".to_string(),
        };

        backend_status
            .provide_core_stats(core_stats, TraceCtx::collect())
            .unwrap();

        // Wait for update interval
        sleep(Duration::from_millis(150)).await;

        // Get the current status and send it
        backend_status.wait_for_updates().await;
        backend_status.send_current_status().await;

        assert_eq!(mock.send_count(), 1);
    }

    #[tokio::test]
    async fn test_multiple_sends() {
        let shutdown_token = CancellationToken::new();
        let mut backend_status =
            BackendStatusImpl::new(Duration::from_millis(100), shutdown_token.clone());
        let mock = MockSender::default();
        backend_status.set_mock_sender(mock.clone());

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

        assert_eq!(mock.send_count(), 3);
    }

    #[tokio::test]
    async fn test_net_stats_handling() {
        let shutdown_token = CancellationToken::new();
        let mut backend_status =
            BackendStatusImpl::new(Duration::from_millis(100), shutdown_token.clone());
        let mock = MockSender::default();
        backend_status.set_mock_sender(mock.clone());

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

        assert_eq!(mock.send_count(), 1);
    }

    #[tokio::test]
    async fn test_multiple_updates() {
        let shutdown_token = CancellationToken::new();
        let mut backend_status =
            BackendStatusImpl::new(Duration::from_millis(100), shutdown_token.clone());
        let mock = MockSender::default();
        backend_status.set_mock_sender(mock.clone());

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
        }];

        backend_status
            .provide_connd_report(ConndReport {
                egress_iface: None,
                wifi_enabled: true,
                smart_switching: true,
                airplane_mode: false,
                active_wifi_profile: Some("test-ssid".into()),
                saved_wifi_profiles: vec![],
                scanned_networks: wifi_networks.clone(),
            })
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
        assert_eq!(mock.send_count(), 1);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let shutdown_token = CancellationToken::new();
        let mut backend_status =
            BackendStatusImpl::new(Duration::from_millis(100), shutdown_token.clone());
        let mock = MockSender::default();
        backend_status.set_mock_sender(mock.clone());

        // Trigger shutdown
        shutdown_token.cancel();

        // Verify that wait_for_updates returns None after shutdown
        backend_status.wait_for_updates().await;
        assert_eq!(mock.send_count(), 0);
    }
}
