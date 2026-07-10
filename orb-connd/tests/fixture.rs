#![cfg(feature = "testing")]
#![allow(dead_code)]
use async_tempfile::TempDir;
use async_trait::async_trait;
use bon::bon;
use color_eyre::Result;
use escargot::CargoBuild;
use faux::when;
use mockall::mock;
use nix::libc;
use orb_connd::{
    connectivity_daemon::program,
    mcu_util::McuUtil,
    modem::ModemConfig,
    modem_manager::{
        connection_state::ConnectionState, Location, Modem, ModemId, ModemInfo,
        ModemManager, Signal, SimInfo,
    },
    network_manager::NetworkManager,
    resolved::Resolved,
    secure_storage::{ConndStorageScopes, SecureStorage},
    service::ProfileStorage,
    systemd::Systemd,
    wpa_ctrl::WpaCtrl,
    OrbCapabilities,
};
use orb_connd_dbus::ConndProxy;
use orb_dogd::{test::agent::Agent, DogstatsdClient};
use orb_info::{
    orb_os_release::{OrbOsPlatform, OrbOsRelease, OrbRelease},
    OrbId,
};
use std::{
    env,
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use test_utils::docker::{self, Container};
use tokio::{fs, task, time};
use tokio_util::sync::CancellationToken;
use zbus::Address;
use zenorb::{zenoh, Zenorb};

pub struct Fixture {
    orb_id: OrbId,
    platform: OrbOsPlatform,
    release: OrbRelease,
    cap: OrbCapabilities,

    wpa_ctrl: Option<MockWpaCli>,
    registry: Option<crabwire::Registry>,

    pub container_tempdir: TempDir,
    sysfs: PathBuf,
    procfs: PathBuf,
    pub usr_persistent: PathBuf,
    connd_bin_path: PathBuf,
}

pub struct FxHandle {
    pub container: Container,

    zenorb: Zenorb,
    zenoh_router_socket: PathBuf,

    dbus: zbus::Connection,
    pub nm: NetworkManager,

    pub secure_storage: SecureStorage,
    secure_storage_cancel_token: CancellationToken,

    pub dogstatsd: Agent,
    dogstatsd_tempdir: TempDir,

    pub speare: speare::mini::Ctx,
}

#[bon]
impl Fixture {
    #[builder(start_fn = platform, finish_fn = build)]
    pub async fn new(
        #[builder(start_fn)] platform: OrbOsPlatform,
        release: OrbRelease,
        #[builder(default = OrbCapabilities::WifiOnly)] cap: OrbCapabilities,
        wpa_ctrl: Option<MockWpaCli>,
        registry: Option<crabwire::Registry>,
    ) -> Self {
        let connd_build = task::spawn_blocking(|| {
            CargoBuild::new()
                .bin("orb-connd")
                .current_target()
                .current_release()
                .manifest_path(env!("CARGO_MANIFEST_PATH"))
                .run()
                .unwrap()
        });

        let container_tempdir = TempDir::new().await.unwrap();
        let usr_persistent = setup_usr_persistent(&container_tempdir).await;
        let sysfs = setup_sysfs(&container_tempdir, cap).await;
        let procfs = setup_procfs(&container_tempdir).await;

        let connd_bin_path = connd_build.await.unwrap().path().to_path_buf();

        Self {
            orb_id: OrbId::from_str("ea2ea744").unwrap(),
            platform,
            release,
            cap,
            wpa_ctrl,
            registry,
            container_tempdir,
            usr_persistent,
            sysfs,
            procfs,
            connd_bin_path,
        }
    }

    pub async fn run_secure_storage(&mut self) -> (SecureStorage, CancellationToken) {
        let secure_storage_cancel_token = CancellationToken::new();
        let secure_storage = SecureStorage::new(
            self.connd_bin_path.clone(),
            true,
            secure_storage_cancel_token.clone(),
            ConndStorageScopes::NmProfiles,
        );

        (secure_storage, secure_storage_cancel_token)
    }

    pub async fn run(&mut self) -> FxHandle {
        self.run_with().log(false).call().await
    }

    #[builder]
    pub async fn run_with(
        &mut self,
        #[builder(default = false)] log: bool,
        secure_storage: Option<SecureStorage>,
        secure_storage_cancel_token: Option<CancellationToken>,
        registry: Option<crabwire::Registry>,
    ) -> FxHandle {
        let _ = color_eyre::install();

        if log {
            let _ = orb_telemetry::TelemetryConfig::new().init();
        }

        let (container, zenoh_router_socket) =
            setup_container(&self.container_tempdir).await;

        time::sleep(Duration::from_secs(1)).await;

        let dbus_socket = container.tempdir.join("socket");
        let dbus_socket = format!("unix:path={}", dbus_socket.display());
        let addr: Address = dbus_socket.parse().unwrap();

        let dbus = zbus::ConnectionBuilder::address(addr)
            .unwrap()
            .build()
            .await
            .unwrap();

        let nm = NetworkManager::new(
            dbus.clone(),
            self.wpa_ctrl.take().unwrap_or_else(default_mock_wpa_cli),
        );

        let (secure_storage, secure_storage_cancel_token) =
            match (secure_storage, secure_storage_cancel_token) {
                (Some(ss), Some(ssct)) => (ss, ssct),

                (None, None) => {
                    let ssct = CancellationToken::new();
                    let ss = SecureStorage::new(
                        self.connd_bin_path.clone(),
                        true,
                        ssct.clone(),
                        ConndStorageScopes::NmProfiles,
                    );

                    (ss, ssct)
                }

                _ => panic!("secure_storage and secure_storage_cancel_token must be both Some or both None"),
            };

        let profile_storage = match self.platform {
            OrbOsPlatform::Pearl => ProfileStorage::NetworkManager,
            OrbOsPlatform::Diamond => {
                ProfileStorage::SecureStorage(secure_storage.clone())
            }
        };

        let zenorb = Zenorb::from_cfg(zenoh_socket_cfg(&zenoh_router_socket))
            .orb_id(self.orb_id.clone())
            .with_name("connd")
            .await
            .unwrap();

        let dogstatsd_tempdir = TempDir::new().await.unwrap();
        let dogstatsd_socket = dogstatsd_tempdir.join("dogstatsd.sock");
        let dogstatsd = Agent::new(&dogstatsd_socket).await.unwrap();
        let statsd = DogstatsdClient::new_with(
            4096,
            25,
            Duration::from_millis(50),
            dogstatsd_socket.to_string_lossy().into_owned(),
            Duration::from_millis(1),
        );

        let base_registry = crabwire::Registry::new()
            .insert(mock_systemd())
            .insert(mock_mcu_util())
            .insert(mock_modem_manager())
            .insert(ModemConfig::default())
            .insert(statsd)
            .merge(self.registry.take().unwrap_or_else(crabwire::Registry::new));

        crabwire::reregister!(base_registry);

        if let Some(registry) = registry {
            crabwire::merge!(registry);
        }

        let speare = program()
            .os_release(OrbOsRelease {
                release_type: self.release,
                orb_os_platform_type: self.platform,
                orb_os_version: String::new(),
                expected_main_mcu_version: String::new(),
                expected_sec_mcu_version: String::new(),
            })
            .network_manager(nm.clone())
            .resolved(Resolved::new(dbus.clone()))
            .sysfs(self.sysfs.clone())
            .procfs(self.procfs.clone())
            .usr_persistent(self.usr_persistent.clone())
            .session_bus(dbus.clone())
            .connect_timeout(Duration::from_secs(1))
            .profile_storage(profile_storage)
            .zenoh(&zenorb)
            .run()
            .await
            .unwrap();

        let millisecs = if env::var("GITHUB_ACTIONS").is_ok() {
            4_000
        } else {
            500
        };

        time::sleep(Duration::from_millis(millisecs)).await;

        FxHandle {
            container,
            zenorb,
            zenoh_router_socket,
            dbus,
            nm,
            secure_storage,
            secure_storage_cancel_token,
            dogstatsd,
            dogstatsd_tempdir,
            speare,
        }
    }
}

impl Drop for FxHandle {
    fn drop(&mut self) {
        self.secure_storage_cancel_token.cancel();
        self.speare.abort_children().unwrap();
    }
}

impl FxHandle {
    pub async fn stop(self) {
        self.secure_storage_cancel_token.cancel();
        self.speare.abort_children().unwrap();
        self.container.rm().await;
    }

    pub async fn connd(&self) -> ConndProxy<'_> {
        ConndProxy::new(&self.dbus).await.unwrap()
    }

    pub fn zenoh(&self) -> &Zenorb {
        &self.zenorb
    }
}

async fn setup_sysfs(container_path: &Path, cap: OrbCapabilities) -> PathBuf {
    let sysfs = container_path.join("sysfs");
    fs::create_dir_all(&sysfs).await.unwrap();

    let net_dir = sysfs.join("class").join("net");
    fs::create_dir_all(net_dir.join("eth0")).await.unwrap();
    fs::create_dir_all(net_dir.join("wlan0")).await.unwrap();

    fs::write(net_dir.join("eth0").join("operstate"), "down\n")
        .await
        .unwrap();
    fs::write(net_dir.join("wlan0").join("operstate"), "up\n")
        .await
        .unwrap();

    if cap == OrbCapabilities::CellularAndWifi {
        let stats = net_dir.join("wwan0").join("statistics");
        let tx = stats.join("tx_bytes");
        let rx = stats.join("rx_bytes");

        fs::create_dir_all(stats).await.unwrap();
        fs::write(tx, "0").await.unwrap();
        fs::write(rx, "0").await.unwrap();

        fs::write(net_dir.join("wwan0").join("operstate"), "unknown\n")
            .await
            .unwrap();
    }

    sysfs
}

async fn setup_procfs(container_path: &Path) -> PathBuf {
    let procfs = container_path.join("procfs");
    fs::create_dir_all(&procfs).await.unwrap();
    let procnet = procfs.join("net");
    let route_path = procnet.join("route");

    fs::create_dir_all(&procnet).await.unwrap();
    fs::write(
        &route_path,
        concat!(
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n",
            "eth0\t0010A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n",
            "wlan0\t00000000\t01006C0A\t0003\t0\t0\t400\t00000000\t0\t0\t0\n",
            "wwan0\t00000000\t39A54664\t0003\t0\t0\t500\t00000000\t0\t0\t0\n",
            "wlan0\t00006C0A\t00000000\t0001\t0\t0\t400\t0000FFFF\t0\t0\t0\n",
            "wwan0\t30A54664\t00000000\t0001\t0\t0\t500\tF0FFFFFF\t0\t0\t0\n",
        ),
    )
    .await
    .unwrap();

    procfs
}

async fn setup_usr_persistent(container_path: &Path) -> PathBuf {
    let usr_persistent = container_path.join("usr_persistent");
    let network_manager_folder = usr_persistent.join("network-manager");
    fs::create_dir_all(&usr_persistent).await.unwrap();
    fs::create_dir_all(&network_manager_folder).await.unwrap();

    usr_persistent
}

async fn setup_container(tempdir: &Path) -> (Container, PathBuf) {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let docker_ctx = crate_dir.join("tests").join("docker");
    let dockerfile = crate_dir.join("tests").join("docker").join("Dockerfile");
    let tag = "worldcoin-nm";
    docker::build(tag, dockerfile, docker_ctx).await;

    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };

    let nm_profiles_dir = tempdir.join("system-connections");
    let zenoh_dir = tempdir.join("zenohd");
    fs::create_dir_all(&nm_profiles_dir).await.unwrap();
    fs::create_dir_all(&zenoh_dir).await.unwrap();

    let target_uid = format!("TARGET_UID={uid}");
    let target_gid = format!("TARGET_GID={gid}");
    let nm_profiles_volume = format!(
        "{}:/etc/NetworkManager/system-connections",
        nm_profiles_dir.display()
    );
    let zenoh_volume = format!("{}:/run/zenohd", zenoh_dir.display());

    let container = docker::run_with(
        tag,
        [
            "--pid=host",
            "--userns=host",
            "-e",
            &target_uid,
            "-e",
            &target_gid,
            "-v",
            &nm_profiles_volume,
            "-v",
            &zenoh_volume,
        ],
        tempdir,
    )
    .await;

    let router_socket = zenoh_dir.join("zenohd.sock");
    wait_for_zenoh_socket(&router_socket).await;

    (container, router_socket)
}

fn zenoh_socket_cfg(socket: &Path) -> zenoh::Config {
    let mut cfg = zenoh::Config::default();
    let endpoint = format!("unixsock-stream/{}", socket.display());
    let endpoints = serde_json::to_string(&[endpoint]).unwrap();

    cfg.insert_json5("mode", r#""client""#).unwrap();
    cfg.insert_json5("connect/endpoints", &endpoints).unwrap();
    cfg.insert_json5("scouting/multicast/enabled", "false")
        .unwrap();

    cfg
}

async fn wait_for_zenoh_socket(socket: &Path) {
    for _ in 0..50 {
        if fs::metadata(socket)
            .await
            .map(|metadata| metadata.file_type().is_socket())
            .unwrap_or(false)
        {
            return;
        }

        time::sleep(Duration::from_millis(100)).await;
    }

    panic!("zenoh socket was not created at {}", socket.display());
}

fn mock_modem_manager() -> ModemManager {
    let mut mm = ModemManager::faux();

    when!(mm.list_modems).then(|_| {
        Ok(vec![Modem {
            id: ModemId::from(0),
            vendor: "telit".to_string(),
            model: "idk i forgot".to_string(),
        }])
    });

    when!(mm.signal_setup).then(|(_, _)| Ok(()));
    when!(mm.signal_get).then(|_| Ok(Signal::default()));
    when!(mm.location_get).then(|_| Ok(Location::default()));

    when!(mm.modem_info).then(|_| {
        let mi = ModemInfo {
            imei: String::new(),
            fw_revision: None,
            operator_code: None,
            operator_name: None,
            access_tech: None,
            state: ConnectionState::Connected,
            sim: None,
        };

        Ok(mi)
    });

    when!(mm.sim_info).then(|_| {
        let si = SimInfo {
            iccid: String::new(),
            imsi: String::new(),
        };

        Ok(si)
    });

    when!(mm.set_current_bands).then(|(_, _)| Ok(()));
    when!(mm.set_allowed_and_preferred_modes).then(|(_, _, _)| Ok(()));

    mm
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

fn mock_mcu_util() -> McuUtil {
    let mut mcu_util = McuUtil::faux();
    when!(mcu_util.powercycle).then(|_| Ok(()));

    mcu_util
}

fn mock_systemd() -> Systemd {
    let mut systemd = Systemd::faux();
    when!(systemd.restart_service).then(|(_, _)| Ok(()));
    when!(systemd.loaded_services).then(|_| Ok(Vec::new()));

    systemd
}
