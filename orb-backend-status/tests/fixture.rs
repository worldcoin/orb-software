use async_tempfile::TempDir;
use color_eyre::Result;
use dbus_launch::BusType;
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use std::{env, path::PathBuf, str::FromStr, time::Duration};
use tokio::{
    fs,
    task::{self, JoinHandle},
    time,
};
use tokio_util::sync::CancellationToken;
use wiremock::MockServer;

/// Sample /proc/net/dev content for tests
const SAMPLE_NET_DEV: &str = r#"Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 351106997 3114910    0    0    0     0          0         0 351106997 3114910    0    0    0     0       0          0
 wlan0: 583824134  881197    0    0    0     0          0         0 992486687  776785    0    0    0     0       0          0
"#;

pub struct Fixture {
    _dbusd: dbus_launch::Daemon,
    _tmpdir: TempDir,
    pub dbus: zbus::Connection,
    endpoint: Url,
    orb_os_version: String,
    orb_id: OrbId,
    orb_name: OrbName,
    orb_jabil_id: OrbJabilId,
    procfs: PathBuf,
    netstats_poll_interval: Duration,
    connectivity_poll_interval: Duration,
    sender_interval: Duration,
    sender_min_backoff: Duration,
    sender_max_backoff: Duration,
    req_timeout: Duration,
    req_min_retry_interval: Duration,
    req_max_retry_interval: Duration,
    shutdown_token: CancellationToken,
    pub mock_server: MockServer,
    pub token_mock: Option<mocks::TokenMock>,
    pub connd_mock: Option<mocks::ConndMock>,
}

#[bon::bon]
impl Fixture {
    pub async fn new() -> Self {
        Fixture::with().build().await
    }

    #[builder(start_fn = with)]
    pub async fn builder(
        #[builder(default = Duration::from_secs(30))] netstats_poll_interval: Duration,
        #[builder(default = Duration::from_millis(100))]
        connectivity_poll_interval: Duration,
        #[builder(default = Duration::from_secs(30))] sender_interval: Duration,
        #[builder(default = Duration::from_secs(1))] sender_min_backoff: Duration,
        #[builder(default = Duration::from_secs(30))] sender_max_backoff: Duration,
        #[builder(default = Duration::from_secs(5))] req_timeout: Duration,
        #[builder(default = Duration::from_millis(100))]
        req_min_retry_interval: Duration,
        #[builder(default = Duration::from_secs(2))] req_max_retry_interval: Duration,
    ) -> Self {
        let shutdown_token = CancellationToken::new();
        let mock_server = MockServer::start().await;

        let dbusd = tokio::task::spawn_blocking(|| {
            dbus_launch::Launcher::daemon()
                .bus_type(BusType::Session)
                .launch()
                .expect("failed to launch dbus-daemon")
        })
        .await
        .expect("task panicked");

        let dbus = zbus::ConnectionBuilder::address(dbusd.address())
            .unwrap()
            .build()
            .await
            .unwrap();

        let tmpdir = TempDir::new().await.unwrap();
        let procfs = tmpdir.to_path_buf();

        let endpoint = mock_server.address().to_string();
        let endpoint = format!("http://{endpoint}").parse().unwrap();

        Fixture {
            _tmpdir: tmpdir,
            _dbusd: dbusd,
            dbus,
            endpoint,
            orb_os_version: "6.6.6".into(),
            orb_id: OrbId::from_str("bba85baa").unwrap(),
            orb_name: OrbName("ota-hilly".into()),
            orb_jabil_id: OrbJabilId("straighttojail".into()),
            procfs,
            mock_server,
            netstats_poll_interval,
            connectivity_poll_interval,
            sender_interval,
            sender_min_backoff,
            sender_max_backoff,
            req_timeout,
            req_min_retry_interval,
            req_max_retry_interval,
            shutdown_token,
            token_mock: None,
            connd_mock: None,
        }
    }

    pub async fn spawn_connected_with_token(sender_interval: Duration) -> Self {
        let mut fx = Fixture::with()
            .sender_interval(sender_interval)
            .build()
            .await;

        fx.setup_procfs().await;
        fx.token_mock = Some(
            mocks::register_token_mock(&fx.dbus, "test-token")
                .await
                .expect("failed to register token mock"),
        );
        fx.connd_mock = Some(
            mocks::register_connd_mock(&fx.dbus, mocks::ConnectionState::Connected)
                .await
                .expect("failed to register connd mock"),
        );

        fx
    }

    pub async fn spawn_connected_without_token(sender_interval: Duration) -> Self {
        let mut fx = Fixture::with()
            .sender_interval(sender_interval)
            .build()
            .await;

        fx.setup_procfs().await;
        fx.token_mock = Some(
            mocks::register_token_mock(&fx.dbus, "") // empty = no token
                .await
                .expect("failed to register token mock"),
        );
        fx.connd_mock = Some(
            mocks::register_connd_mock(&fx.dbus, mocks::ConnectionState::Connected)
                .await
                .expect("failed to register connd mock"),
        );

        fx
    }

    pub async fn spawn_disconnected_with_token(sender_interval: Duration) -> Self {
        let mut fx = Fixture::with()
            .sender_interval(sender_interval)
            .build()
            .await;

        fx.setup_procfs().await;
        fx.token_mock = Some(
            mocks::register_token_mock(&fx.dbus, "test-token")
                .await
                .expect("failed to register token mock"),
        );
        fx.connd_mock = Some(
            mocks::register_connd_mock(&fx.dbus, mocks::ConnectionState::Disconnected)
                .await
                .expect("failed to register connd mock"),
        );

        fx
    }

    pub async fn spawn_disconnected_without_token(sender_interval: Duration) -> Self {
        let mut fx = Fixture::with()
            .sender_interval(sender_interval)
            .build()
            .await;

        fx.setup_procfs().await;
        fx.token_mock = Some(
            mocks::register_token_mock(&fx.dbus, "")
                .await
                .expect("failed to register token mock"),
        );
        fx.connd_mock = Some(
            mocks::register_connd_mock(&fx.dbus, mocks::ConnectionState::Disconnected)
                .await
                .expect("failed to register connd mock"),
        );

        fx
    }

    /// Set up fake /proc/net/dev to prevent net_stats errors.
    async fn setup_procfs(&self) {
        let net_dir = self.procfs.join("net");
        fs::create_dir_all(&net_dir)
            .await
            .expect("failed to create procfs net dir");
        fs::write(net_dir.join("dev"), SAMPLE_NET_DEV)
            .await
            .expect("failed to write fake net/dev");
    }

    pub async fn start(&self) -> JoinHandle<Result<()>> {
        let program = orb_backend_status::program()
            .dbus(self.dbus.clone())
            .endpoint(self.endpoint.clone())
            .orb_os_version(self.orb_os_version.clone())
            .orb_id(self.orb_id.clone())
            .orb_name(self.orb_name.clone())
            .orb_jabil_id(self.orb_jabil_id.clone())
            .procfs(self.procfs.clone())
            .net_stats_poll_interval(self.netstats_poll_interval)
            .connectivity_poll_interval(self.connectivity_poll_interval)
            .sender_interval(self.sender_interval)
            .sender_min_backoff(self.sender_min_backoff)
            .sender_max_backoff(self.sender_max_backoff)
            .req_timeout(self.req_timeout)
            .req_min_retry_interval(self.req_min_retry_interval)
            .req_max_retry_interval(self.req_max_retry_interval)
            .shutdown_token(self.shutdown_token.clone());

        let task = task::spawn(async move {
            program
                .run()
                .await
                .inspect_err(|e| println!("program failed: {e}"))
        });

        let secs = if env::var("GITHUB_ACTIONS").is_ok() {
            5
        } else {
            1
        };

        time::sleep(Duration::from_secs(secs)).await;

        task
    }

    pub fn stop(&self) {
        self.shutdown_token.cancel();
    }

    #[allow(dead_code)]
    pub fn log(&self) -> &Self {
        let _ = orb_telemetry::TelemetryConfig::new().init();
        self
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}

pub mod mocks {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use zbus::zvariant::Value;
    use zbus::{interface, Connection};

    use orb_backend_status_dbus::types::UpdateProgress;
    pub use orb_update_agent_dbus::UpdateAgentState;

    const AUTH_TOKEN_SERVICE: &str = "org.worldcoin.AuthTokenManager1";
    const AUTH_TOKEN_PATH: &str = "/org/worldcoin/AuthTokenManager1";

    pub struct MockAuthTokenManager {
        token: Arc<Mutex<String>>,
    }

    #[interface(name = "org.worldcoin.AuthTokenManager1")]
    impl MockAuthTokenManager {
        #[zbus(property)]
        fn token(&self) -> String {
            self.token.lock().unwrap().clone()
        }

        #[zbus(property)]
        fn set_token(&self, value: String) {
            *self.token.lock().unwrap() = value;
        }
    }

    pub struct TokenMock {
        token: Arc<Mutex<String>>,
        connection: Connection,
    }

    impl TokenMock {
        pub async fn update_token(&self, new_token: &str) -> zbus::Result<()> {
            let iface_ref = self
                .connection
                .object_server()
                .interface::<_, MockAuthTokenManager>(AUTH_TOKEN_PATH)
                .await?;

            let signal_ctx =
                zbus::SignalContext::new(&self.connection, AUTH_TOKEN_PATH)?;

            {
                let iface = iface_ref.get().await;
                *iface.token.lock().unwrap() = new_token.to_string();
                MockAuthTokenManager::token_changed(&iface, &signal_ctx).await?;
            }

            Ok(())
        }

        #[allow(dead_code)]
        pub fn set_token(&self, new_token: &str) {
            *self.token.lock().unwrap() = new_token.to_string();
        }
    }

    pub async fn register_token_mock(
        connection: &Connection,
        initial_token: &str,
    ) -> zbus::Result<TokenMock> {
        let token = Arc::new(Mutex::new(initial_token.to_string()));
        let mock = MockAuthTokenManager {
            token: token.clone(),
        };

        connection.request_name(AUTH_TOKEN_SERVICE).await?;
        connection.object_server().at(AUTH_TOKEN_PATH, mock).await?;

        Ok(TokenMock {
            token,
            connection: connection.clone(),
        })
    }

    const CONND_SERVICE: &str = "org.worldcoin.Connd";
    const CONND_PATH: &str = "/org/worldcoin/Connd1";

    #[derive(Debug, Clone, Copy, PartialEq)]
    #[repr(u32)]
    pub enum ConnectionState {
        Disconnected = 0,
        Disconnecting = 1,
        Connecting = 2,
        PartiallyConnected = 3,
        Connected = 4,
    }

    impl serde::Serialize for ConnectionState {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.serialize_u32(*self as u32)
        }
    }

    impl<'de> serde::Deserialize<'de> for ConnectionState {
        fn deserialize<D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Self, D::Error> {
            let value = u32::deserialize(deserializer)?;
            match value {
                0 => Ok(Self::Disconnected),
                1 => Ok(Self::Disconnecting),
                2 => Ok(Self::Connecting),
                3 => Ok(Self::PartiallyConnected),
                4 => Ok(Self::Connected),
                _ => Err(serde::de::Error::custom("invalid connection state")),
            }
        }
    }

    impl zbus::zvariant::Type for ConnectionState {
        fn signature() -> zbus::zvariant::Signature<'static> {
            u32::signature()
        }
    }

    pub struct MockConnd {
        state: Arc<Mutex<ConnectionState>>,
    }

    #[interface(name = "org.worldcoin.Connd1")]
    impl MockConnd {
        fn connection_state(&self) -> ConnectionState {
            *self.state.lock().unwrap()
        }
    }

    pub struct ConndMock {
        state: Arc<Mutex<ConnectionState>>,
    }

    impl ConndMock {
        pub fn set_connected(&self) {
            *self.state.lock().unwrap() = ConnectionState::Connected;
        }

        #[allow(dead_code)]
        pub fn set_disconnected(&self) {
            *self.state.lock().unwrap() = ConnectionState::Disconnected;
        }
    }

    pub async fn register_connd_mock(
        connection: &Connection,
        initial_state: ConnectionState,
    ) -> zbus::Result<ConndMock> {
        let state = Arc::new(Mutex::new(initial_state));
        let mock = MockConnd {
            state: state.clone(),
        };

        connection.request_name(CONND_SERVICE).await?;
        connection.object_server().at(CONND_PATH, mock).await?;

        Ok(ConndMock { state })
    }

    const BACKEND_STATUS_SERVICE: &str = "org.worldcoin.BackendStatus1";
    const BACKEND_STATUS_PATH: &str = "/org/worldcoin/BackendStatus1";
    const BACKEND_STATUS_IFACE: &str = "org.worldcoin.BackendStatus1";

    #[derive(Debug, Clone, Copy)]
    #[repr(u32)]
    #[allow(dead_code)]
    pub enum SignupState {
        Unknown = 0,
        Ready = 1,
        NotReady = 2,
        InProgress = 3,
        CompletedSuccess = 4,
        CompletedFailure = 5,
    }

    fn empty_trace_ctx() -> HashMap<&'static str, Value<'static>> {
        let inner_ctx: HashMap<String, String> = HashMap::new();
        let mut trace_ctx: HashMap<&str, Value> = HashMap::new();
        trace_ctx.insert("ctx", Value::new(inner_ctx));
        trace_ctx
    }

    pub async fn trigger_update_progress_rebooting(
        connection: &Connection,
    ) -> zbus::Result<()> {
        let progress = UpdateProgress {
            download_progress: 100,
            processed_progress: 100,
            install_progress: 100,
            total_progress: 100,
            error: None,
            state: UpdateAgentState::Rebooting,
        };

        connection
            .call_method(
                Some(BACKEND_STATUS_SERVICE),
                BACKEND_STATUS_PATH,
                Some(BACKEND_STATUS_IFACE),
                "ProvideUpdateProgress",
                &(progress, empty_trace_ctx()),
            )
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn provide_update_progress(
        connection: &Connection,
        state: UpdateAgentState,
        download_progress: u64,
    ) -> zbus::Result<()> {
        let progress = UpdateProgress {
            download_progress,
            processed_progress: 0,
            install_progress: 0,
            total_progress: download_progress / 4,
            error: None,
            state,
        };

        connection
            .call_method(
                Some(BACKEND_STATUS_SERVICE),
                BACKEND_STATUS_PATH,
                Some(BACKEND_STATUS_IFACE),
                "ProvideUpdateProgress",
                &(progress, empty_trace_ctx()),
            )
            .await?;

        Ok(())
    }

    pub async fn provide_connd_report(
        connection: &Connection,
        active_wifi_profile: Option<&str>,
    ) -> zbus::Result<()> {
        let report = (
            Option::<String>::None,                // egress_iface
            true,                                  // wifi_enabled
            false,                                 // smart_switching
            false,                                 // airplane_mode
            active_wifi_profile.map(String::from), // active_wifi_profile
            Vec::<(String, String)>::new(),        // saved_wifi_profiles
            Vec::<HashMap<&str, Value>>::new(),    // scanned_networks
        );

        connection
            .call_method(
                Some(BACKEND_STATUS_SERVICE),
                BACKEND_STATUS_PATH,
                Some(BACKEND_STATUS_IFACE),
                "ProvideConndReport",
                &report,
            )
            .await?;

        Ok(())
    }

    pub async fn provide_signup_state(
        connection: &Connection,
        state: SignupState,
    ) -> zbus::Result<()> {
        connection
            .call_method(
                Some(BACKEND_STATUS_SERVICE),
                BACKEND_STATUS_PATH,
                Some(BACKEND_STATUS_IFACE),
                "ProvideSignupState",
                &(state as u32, empty_trace_ctx()),
            )
            .await?;

        Ok(())
    }

    pub async fn provide_cellular_status(
        connection: &Connection,
        imei: &str,
        operator: Option<&str>,
    ) -> zbus::Result<()> {
        let status = (
            imei.to_string(),           // imei
            Option::<String>::None,     // iccid
            Some("lte".to_string()),    // rat
            operator.map(String::from), // operator
            Some(-90.0_f64),            // rsrp
            Some(-10.0_f64),            // rsrq
            Some(-70.0_f64),            // rssi
            Some(15.0_f64),             // snr
        );

        connection
            .call_method(
                Some(BACKEND_STATUS_SERVICE),
                BACKEND_STATUS_PATH,
                Some(BACKEND_STATUS_IFACE),
                "ProvideCellularStatus",
                &status,
            )
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn provide_core_stats(
        connection: &Connection,
        battery_level: f64,
        cpu_temp: f64,
    ) -> zbus::Result<()> {
        let battery: HashMap<&str, Value> = [
            ("level", Value::F64(battery_level)),
            ("is_charging", Value::Bool(true)),
        ]
        .into_iter()
        .collect();

        let mut temperature: HashMap<&str, Value> = HashMap::new();
        temperature.insert("cpu", Value::F64(cpu_temp));
        temperature.insert("gpu", Value::F64(45.0));
        temperature.insert("front_unit", Value::F64(30.0));
        temperature.insert("front_pcb", Value::F64(30.0));
        temperature.insert("backup_battery", Value::F64(25.0));
        temperature.insert("battery_pcb", Value::F64(25.0));
        temperature.insert("battery_cell", Value::F64(25.0));
        temperature.insert("liquid_lens", Value::F64(25.0));
        temperature.insert("main_accelerometer", Value::F64(25.0));
        temperature.insert("main_mcu", Value::F64(25.0));
        temperature.insert("mainboard", Value::F64(35.0));
        temperature.insert("security_accelerometer", Value::F64(25.0));
        temperature.insert("security_mcu", Value::F64(25.0));
        temperature.insert("battery_pack", Value::F64(25.0));
        temperature.insert("ssd", Value::F64(40.0));
        temperature.insert("wifi", Value::F64(35.0));
        temperature.insert("main_board_usb_hub_bot", Value::F64(30.0));
        temperature.insert("main_board_usb_hub_top", Value::F64(30.0));
        temperature.insert("main_board_security_supply", Value::F64(30.0));
        temperature.insert("main_board_audio_amplifier", Value::F64(30.0));
        temperature.insert("power_board_super_cap_charger", Value::F64(30.0));
        temperature.insert("power_board_pvcc_supply", Value::F64(30.0));
        temperature.insert("power_board_super_caps_left", Value::F64(30.0));
        temperature.insert("power_board_super_caps_right", Value::F64(30.0));
        temperature.insert("front_unit_850_730_left_top", Value::F64(30.0));
        temperature.insert("front_unit_850_730_left_bottom", Value::F64(30.0));
        temperature.insert("front_unit_850_730_right_top", Value::F64(30.0));
        temperature.insert("front_unit_850_730_right_bottom", Value::F64(30.0));
        temperature.insert("front_unit_940_left_top", Value::F64(30.0));
        temperature.insert("front_unit_940_left_bottom", Value::F64(30.0));
        temperature.insert("front_unit_940_right_top", Value::F64(30.0));
        temperature.insert("front_unit_940_right_bottom", Value::F64(30.0));
        temperature.insert("front_unit_940_center_top", Value::F64(30.0));
        temperature.insert("front_unit_940_center_bottom", Value::F64(30.0));
        temperature.insert("front_unit_white_top", Value::F64(30.0));
        temperature.insert("front_unit_shroud_rgb_top", Value::F64(30.0));

        let location: HashMap<&str, Value> = [
            ("latitude", Value::F64(0.0)),
            ("longitude", Value::F64(0.0)),
        ]
        .into_iter()
        .collect();

        let ssd: HashMap<&str, Value> = [
            ("file_left", Value::I64(1000)),
            ("space_left", Value::I64(50_000_000_000)),
            ("signup_left_to_upload", Value::I64(5)),
        ]
        .into_iter()
        .collect();

        let version: HashMap<&str, Value> =
            [("current_release", Value::Str("1.0.0".into()))]
                .into_iter()
                .collect();

        let wifi: Vec<HashMap<&str, Value>> = vec![];

        let stats = (
            battery,
            wifi,
            temperature,
            location,
            ssd,
            version,
            "00:11:22:33:44:55".to_string(),
        );

        connection
            .call_method(
                Some(BACKEND_STATUS_SERVICE),
                BACKEND_STATUS_PATH,
                Some(BACKEND_STATUS_IFACE),
                "ProvideCoreStats",
                &(stats, empty_trace_ctx()),
            )
            .await?;

        Ok(())
    }
}
