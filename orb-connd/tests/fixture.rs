#![allow(dead_code)]
use async_trait::async_trait;
use bon::bon;
use color_eyre::Result;
use escargot::CargoBuild;
use mockall::mock;
use nix::libc;
use orb_connd::{
    connectivity_daemon::program,
    modem_manager::{
        connection_state::ConnectionState, Location, Modem, ModemId, ModemInfo,
        ModemManager, Signal, SimId, SimInfo,
    },
    network_manager::NetworkManager,
    secure_storage::{ConndStorageScopes, SecureStorage},
    service::ProfileStorage,
    statsd::StatsdClient,
    wpa_ctrl::WpaCtrl,
    OrbCapabilities,
};
use orb_connd_dbus::ConndProxy;
use orb_info::{
    orb_os_release::{OrbOsPlatform, OrbOsRelease, OrbRelease},
    OrbId,
};
use prelude::future::Callback;
use std::{env, path::PathBuf, str::FromStr, time::Duration};
use test_utils::docker::{self, Container};
use tokio::{fs, task::JoinHandle, time};
use tokio_util::sync::CancellationToken;
use zbus::Address;
use zenorb::{zenoh, Zenorb};

#[allow(dead_code)]
pub struct Fixture {
    pub nm: NetworkManager,
    pub container: Container,
    conn: zbus::Connection,
    program_handles: Vec<JoinHandle<Result<()>>>,
    pub sysfs: PathBuf,
    pub usr_persistent: PathBuf,
    pub secure_storage: SecureStorage,
    pub secure_storage_cancel_token: CancellationToken,
    zsession: Zenorb,
    router_port: u16,
    pub orb_id: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.secure_storage_cancel_token.cancel();

        for handle in &self.program_handles {
            handle.abort();
        }
    }
}

#[allow(dead_code)]
pub struct Ctx {
    pub usr_persistent: PathBuf,
    pub nm: NetworkManager,
    pub secure_storage: SecureStorage,
}

#[bon]
impl Fixture {
    #[builder(start_fn = platform, finish_fn = run)]
    pub async fn new(
        #[builder(start_fn)] platform: OrbOsPlatform,
        release: OrbRelease,
        #[builder(default = OrbCapabilities::WifiOnly)] cap: OrbCapabilities,
        modem_manager: Option<MockMMCli>,
        statsd: Option<MockStatsd>,
        wpa_ctrl: Option<MockWpaCli>,
        arrange: Option<Callback<Ctx>>,
        #[builder(default = false)] log: bool,
    ) -> Self {
        let _ = color_eyre::install();

        if log {
            let _ = orb_telemetry::TelemetryConfig::new().init();
        }

        let (container, router_port) = setup_container().await;
        let sysfs = container.tempdir.path().join("sysfs");
        let usr_persistent = container.tempdir.path().join("usr_persistent");
        let network_manager_folder = usr_persistent.join("network-manager");
        fs::create_dir_all(&sysfs).await.unwrap();
        fs::create_dir_all(&usr_persistent).await.unwrap();
        fs::create_dir_all(&network_manager_folder).await.unwrap();

        if cap == OrbCapabilities::CellularAndWifi {
            let stats = sysfs
                .join("class")
                .join("net")
                .join("wwan0")
                .join("statistics");

            let tx = stats.join("tx_bytes");
            let rx = stats.join("rx_bytes");

            fs::create_dir_all(stats).await.unwrap();
            fs::write(tx, "0").await.unwrap();
            fs::write(rx, "0").await.unwrap();
        }

        time::sleep(Duration::from_secs(1)).await;

        let dbus_socket = container.tempdir.path().join("socket");
        let dbus_socket = format!("unix:path={}", dbus_socket.display());
        let addr: Address = dbus_socket.parse().unwrap();

        let conn = zbus::ConnectionBuilder::address(addr)
            .unwrap()
            .build()
            .await
            .unwrap();

        let nm = NetworkManager::new(
            conn.clone(),
            wpa_ctrl.unwrap_or_else(default_mock_wpa_cli),
        );

        let built_connd = CargoBuild::new()
            .bin("orb-connd")
            .current_target()
            .current_release()
            .manifest_path(env!("CARGO_MANIFEST_PATH"))
            .run()
            .unwrap();

        let cancel_token = CancellationToken::new();
        let secure_storage = SecureStorage::new(
            built_connd.path().into(),
            true,
            cancel_token.clone(),
            ConndStorageScopes::NmProfiles,
        );

        let profile_storage = match platform {
            OrbOsPlatform::Pearl => ProfileStorage::NetworkManager,
            OrbOsPlatform::Diamond => {
                ProfileStorage::SecureStorage(secure_storage.clone())
            }
        };

        if let Some(arrange_cb) = arrange {
            let ctx = Ctx {
                usr_persistent: usr_persistent.clone(),
                nm: nm.clone(),
                secure_storage: secure_storage.clone(),
            };

            arrange_cb.call(ctx).await;
        }
        let orb_id = OrbId::from_str("ea2ea744").unwrap();
        let zsession = Zenorb::from_cfg(zenorb::client_cfg(router_port))
            .orb_id(orb_id.clone())
            .with_name("connd")
            .await
            .unwrap();

        let program_handles = program()
            .os_release(OrbOsRelease {
                release_type: release,
                orb_os_platform_type: platform,
                expected_main_mcu_version: String::new(),
                expected_sec_mcu_version: String::new(),
            })
            .modem_manager(modem_manager.unwrap_or_else(default_mockmmcli))
            .network_manager(nm.clone())
            .statsd_client(statsd.unwrap_or(MockStatsd))
            .sysfs(sysfs.clone())
            .usr_persistent(usr_persistent.clone())
            .session_bus(conn.clone())
            .connect_timeout(Duration::from_secs(1))
            .profile_storage(profile_storage)
            .zenoh(&zsession)
            .run()
            .await
            .unwrap();

        let millisecs = if env::var("GITHUB_ACTIONS").is_ok() {
            4_000
        } else {
            500
        };

        time::sleep(Duration::from_millis(millisecs)).await;

        Self {
            nm,
            conn,
            program_handles,
            container,
            sysfs,
            usr_persistent,
            secure_storage,
            secure_storage_cancel_token: cancel_token,
            router_port,
            zsession,
            orb_id: orb_id.to_string(),
        }
    }

    pub async fn connd(&self) -> ConndProxy<'_> {
        ConndProxy::new(&self.conn).await.unwrap()
    }

    pub async fn zenoh(&self) -> zenoh::Session {
        zenoh::open(zenorb::client_cfg(self.router_port))
            .await
            .unwrap()
    }
}

async fn setup_container() -> (Container, u16) {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let docker_ctx = crate_dir.join("tests").join("docker");
    let dockerfile = crate_dir.join("tests").join("docker").join("Dockerfile");
    let tag = "worldcoin-nm";
    docker::build(tag, dockerfile, docker_ctx).await;

    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };

    let zenohport = portpicker::pick_unused_port().expect("No ports free");

    let container = docker::run(
        tag,
        [
            "--pid=host",
            "--userns=host",
            "-e",
            &format!("TARGET_UID={uid}"),
            "-e",
            &format!("TARGET_GID={gid}"),
            &format!("-p={zenohport}:7447"),
        ],
    )
    .await;

    (container, zenohport)
}

fn default_mockmmcli() -> MockMMCli {
    let mut mm = MockMMCli::new();

    mm.expect_list_modems().returning(|| {
        Ok(vec![Modem {
            id: ModemId::from(0),
            vendor: "telit".to_string(),
            model: "idk i forgot".to_string(),
        }])
    });

    mm.expect_signal_setup().returning(|_, _| Ok(()));

    mm.expect_signal_get().returning(|_| Ok(Signal::default()));

    mm.expect_location_get()
        .returning(|_| Ok(Location::default()));

    mm.expect_modem_info().returning(|_| {
        let mi = ModemInfo {
            imei: String::new(),
            operator_code: None,
            operator_name: None,
            access_tech: None,
            state: ConnectionState::Connected,
            sim: None,
        };

        Ok(mi)
    });

    mm.expect_sim_info().returning(|_| {
        let si = SimInfo {
            iccid: String::new(),
            imsi: String::new(),
        };

        Ok(si)
    });

    mm.expect_set_current_bands().returning(|_, _| Ok(()));
    mm.expect_set_allowed_and_preferred_modes()
        .returning(|_, _, _| Ok(()));

    mm
}

mock! {
    pub MMCli {}
    #[async_trait]
    impl ModemManager for MMCli {
        async fn list_modems(&self) -> Result<Vec<Modem>>;

        async fn modem_info(&self, modem_id: &ModemId) -> Result<ModemInfo>;

        async fn signal_setup(&self, modem_id: &ModemId, rate: Duration) -> Result<()>;

        async fn signal_get(&self, modem_id: &ModemId) -> Result<Signal>;

        async fn location_get(&self, modem_id: &ModemId) -> Result<Location>;

        async fn sim_info(&self, sim_id: &SimId) -> Result<SimInfo>;

        async fn set_current_bands<'a>(&self, modem_id: &ModemId, bands: &[&'a str])
            -> Result<()>;

        async fn set_allowed_and_preferred_modes<'a>(
            &self,
            modem_id: &ModemId,
            allowed: &[&'a str],
            preferred: &'a str,
        ) -> Result<()>;
    }
}

pub struct MockStatsd;

impl StatsdClient for MockStatsd {
    async fn count<S: AsRef<str> + Sync + Send>(
        &self,
        _stat: &str,
        _count: i64,
        _tags: &[S],
    ) -> Result<()> {
        Ok(())
    }

    async fn incr_by_value<S: AsRef<str> + Sync + Send>(
        &self,
        _stat: &str,
        _value: i64,
        _tags: &[S],
    ) -> Result<()> {
        Ok(())
    }

    async fn gauge<S: AsRef<str> + Sync + Send>(
        &self,
        _stat: &str,
        _val: &str,
        _tags: &[S],
    ) -> Result<()> {
        Ok(())
    }
}

fn default_mock_wpa_cli() -> MockWpaCli {
    let mut wpa = MockWpaCli::new();
    wpa.expect_scan_results().returning(|| Ok(Vec::new()));

    wpa
}

mock! {
    pub WpaCli {}
    #[async_trait]
    impl WpaCtrl for WpaCli {
        async fn scan_results(&self) -> Result<Vec<orb_connd::wpa_ctrl::AccessPoint>>;
    }
}
