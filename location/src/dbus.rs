use eyre::Result;
use orb_backend_status_dbus::{types::WifiNetwork, BackendStatusProxy};
use orb_telemetry::TraceCtx;
use tracing::instrument;
use zbus::Connection;

use crate::data::NetworkInfo;

#[derive(Debug)]
pub struct BackendStatus {
    backend_status_proxy: BackendStatusProxy<'static>,
}

impl BackendStatus {
    pub async fn new(connection: &Connection) -> Result<Self> {
        let backend_status_proxy = BackendStatusProxy::new(connection).await?;
        Ok(Self {
            backend_status_proxy,
        })
    }

    #[instrument]
    pub async fn send_location_data(&self, network_info: &NetworkInfo) -> Result<()> {
        let dbus_wifi_networks = network_info
            .wifi
            .iter()
            .map(|wifi| WifiNetwork {
                bssid: wifi.bssid.clone(),
                frequency: wifi.frequency,
                signal_level: wifi.signal_level,
                flags: wifi.flags.clone(),
                ssid: wifi.ssid.clone(),
            })
            .collect();

        self.backend_status_proxy
            .provide_wifi_networks(dbus_wifi_networks, TraceCtx::collect())
            .await?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    use eyre::Result;
    use orb_backend_status_dbus::{
        types::{CoreStats, NetStats, SignupState, UpdateProgress},
        BackendStatusT,
    };
    use std::sync::{Arc, Mutex};
    use zbus::ConnectionBuilder;

    #[derive(Clone, Debug)]
    struct TestBackendStatus {
        received_networks: Arc<Mutex<Option<Vec<WifiNetwork>>>>,
    }

    impl BackendStatusT for TestBackendStatus {
        fn provide_wifi_networks(
            &self,
            networks: Vec<WifiNetwork>,
            _trace_ctx: TraceCtx,
        ) -> zbus::fdo::Result<()> {
            *self.received_networks.lock().unwrap() = Some(networks);
            Ok(())
        }

        fn provide_update_progress(
            &self,
            _update_progress: UpdateProgress,
            _trace_ctx: TraceCtx,
        ) -> zbus::fdo::Result<()> {
            Ok(())
        }

        fn provide_net_stats(
            &self,
            _net_stats: NetStats,
            _trace_ctx: TraceCtx,
        ) -> zbus::fdo::Result<()> {
            Ok(())
        }

        fn provide_cellular_status(
            &self,
            _status: orb_backend_status_dbus::types::CellularStatus,
        ) -> zbus::fdo::Result<()> {
            Ok(())
        }

        fn provide_core_stats(
            &self,
            _core_stats: CoreStats,
            _trace_ctx: TraceCtx,
        ) -> zbus::fdo::Result<()> {
            Ok(())
        }

        fn provide_signup_state(
            &self,
            _signup_state: SignupState,
            _trace_ctx: TraceCtx,
        ) -> zbus::fdo::Result<()> {
            Ok(())
        }
    }

    // using `dbus_launch` ensures that all tests use their own isolated dbus, and that they can't influence each other.
    async fn start_dbus_daemon() -> dbus_launch::Daemon {
        tokio::task::spawn_blocking(|| {
            dbus_launch::Launcher::daemon()
                .listen("tcp:host=localhost")
                .launch()
                .expect("failed to launch dbus-daemon")
        })
        .await
        .expect("task panicked")
    }

    async fn setup_test_server(
    ) -> Result<(Connection, dbus_launch::Daemon, TestBackendStatus)> {
        let received_networks = Arc::new(Mutex::new(None));
        let mock_manager = TestBackendStatus { received_networks };
        let daemon = start_dbus_daemon().await;

        let connection = ConnectionBuilder::address(daemon.address())?
            .name(orb_backend_status_dbus::constants::SERVICE_NAME)?
            .serve_at(
                orb_backend_status_dbus::constants::OBJECT_PATH,
                orb_backend_status_dbus::BackendStatus(mock_manager.clone()),
            )?
            .build()
            .await?;

        Ok((connection, daemon, mock_manager))
    }

    #[tokio::test]
    async fn test_send_location_data() -> Result<()> {
        let (connection, _daemon, test_service) = setup_test_server().await?;
        let backend_status = BackendStatus::new(&connection).await?;

        // Create test data
        let network_info = NetworkInfo {
            wifi: vec![
                crate::data::WifiNetwork {
                    bssid: "00:11:22:33:44:55".to_string(),
                    frequency: 2412,
                    signal_level: -50,
                    flags: "[WPA2-PSK-CCMP]".to_string(),
                    ssid: "TestNetwork".to_string(),
                },
                crate::data::WifiNetwork {
                    bssid: "AA:BB:CC:DD:EE:FF".to_string(),
                    frequency: 5180,
                    signal_level: -65,
                    flags: "[WPA2-PSK-CCMP][ESS]".to_string(),
                    ssid: "TestNetwork5G".to_string(),
                },
            ],
        };

        // Send the data
        backend_status.send_location_data(&network_info).await?;

        // Give some time for the D-Bus message to be processed
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify the received data
        let received = test_service.received_networks.lock().unwrap().clone();
        assert!(received.is_some());

        let received_networks = received.unwrap();
        assert_eq!(received_networks.len(), 2);

        assert_eq!(received_networks[0].bssid, "00:11:22:33:44:55");
        assert_eq!(received_networks[0].frequency, 2412);
        assert_eq!(received_networks[0].signal_level, -50);
        assert_eq!(received_networks[0].flags, "[WPA2-PSK-CCMP]");
        assert_eq!(received_networks[0].ssid, "TestNetwork");

        assert_eq!(received_networks[1].bssid, "AA:BB:CC:DD:EE:FF");
        assert_eq!(received_networks[1].frequency, 5180);
        assert_eq!(received_networks[1].signal_level, -65);
        assert_eq!(received_networks[1].flags, "[WPA2-PSK-CCMP][ESS]");
        assert_eq!(received_networks[1].ssid, "TestNetwork5G");

        connection.close().await?;
        Ok(())
    }
}
