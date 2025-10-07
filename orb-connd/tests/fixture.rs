use bon::bon;
use color_eyre::Result;
use mockall::mock;
use nix::libc;
use orb_connd::{
    modem_manager::{
        Location, Modem, ModemId, ModemInfo, ModemManager, Signal, SimId, SimInfo,
    },
    network_manager::NetworkManager,
    program,
    service::ConndService,
    statsd::StatsdClient,
};
use orb_connd_dbus::ConndProxy;
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease, OrbRelease};
use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};
use test_utils::docker::{self, Container};
use tokio::{
    fs,
    task::{self, JoinHandle},
    time,
};
use zbus::Address;

#[allow(dead_code)]
pub struct Fixture {
    pub nm: NetworkManager,
    container: Container,
    conn: zbus::Connection,
    program_handle: JoinHandle<Result<()>>,
    pub sysfs_path: PathBuf,
    pub wpa_conf_path: PathBuf,
}

pub struct ArrangeCtx<'a> {
    pub sysfs: &'a Path,
    pub wpa_conf: &'a Path,
    pub mm: &'a mut MockMMCli,
}

#[bon]
impl Fixture {
    #[builder(finish_fn = run)]
    pub async fn new(
        #[builder(start_fn)] platform: OrbOsPlatform,
        release: OrbRelease,
        arrange: Option<impl AsyncFnOnce(ArrangeCtx<'_>)>,
    ) -> Self {
        let container = setup_container().await;
        let sysfs_path = container.tempdir.path().join("sysfs");
        let wpa_conf_path = container.tempdir.path().join("wpaconf");
        fs::create_dir_all(&sysfs_path).await.unwrap();
        fs::create_dir_all(&wpa_conf_path).await.unwrap();

        time::sleep(Duration::from_secs(1)).await;

        let mut mm = MockMMCli::new();

        if let Some(arrange) = arrange {
            let ctx = ArrangeCtx {
                sysfs: sysfs_path.as_ref(),
                wpa_conf: wpa_conf_path.as_ref(),
                mm: &mut mm,
            };

            arrange(ctx).await;
        }

        let dbus_socket = container.tempdir.path().join("socket");
        let dbus_socket = format!("unix:path={}", dbus_socket.display());
        let addr: Address = dbus_socket.parse().unwrap();

        // todo: retry for
        let conn = zbus::ConnectionBuilder::address(addr)
            .unwrap()
            .build()
            .await
            .unwrap();

        let program_handle = task::spawn(
            program()
                .os_release(OrbOsRelease {
                    release_type: release,
                    orb_os_platform_type: platform,
                    expected_main_mcu_version: String::new(),
                    expected_sec_mcu_version: String::new(),
                })
                .modem_manager(MockMMCli::new())
                .statsd_client(MockStatsd)
                .sysfs(sysfs_path.clone())
                .wpa_conf_dir(wpa_conf_path.clone())
                .session_bus(conn.clone())
                .system_bus(conn.clone())
                .run(),
        );

        let secs = if env::var("GITHUB_ACTIONS").is_ok() {
            5
        } else {
            1
        };

        time::sleep(Duration::from_secs(secs)).await;

        Self {
            nm: NetworkManager::new(conn.clone()),
            conn,
            program_handle,
            container,
            sysfs_path,
            wpa_conf_path,
        }
    }

    pub async fn connd(&self) -> ConndProxy<'_> {
        ConndProxy::new(&self.conn).await.unwrap()
    }
}

async fn setup_container() -> Container {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let docker_ctx = crate_dir.join("tests").join("docker");
    let dockerfile = crate_dir.join("tests").join("docker").join("Dockerfile");
    let tag = "worldcoin-nm";
    docker::build("worldcoin-nm", dockerfile, docker_ctx).await;

    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };

    docker::run(
        tag,
        [
            "--pid=host",
            "--userns=host",
            "-e",
            &format!("TARGET_UID={uid}"),
            "-e",
            &format!("TARGET_GID={gid}"),
        ],
    )
    .await
}

mock! {
    MMCli {}
    impl ModemManager for MMCli {
        fn list_modems(&self) -> impl Future<Output = Result<Vec<Modem>>> + Send + Sync;

        fn modem_info(
            &self,
            modem_id: &ModemId,
        ) -> impl Future<Output = Result<ModemInfo>> + Send + Sync;

        fn signal_setup(
            &self,
            modem_id: &ModemId,
            rate: Duration,
        ) -> impl Future<Output = Result<()>> + Send + Sync;

        fn signal_get(
            &self,
            modem_id: &ModemId,
        ) -> impl Future<Output = Result<Signal>> + Send + Sync;

        fn location_get(
            &self,
            modem_id: &ModemId,
        ) -> impl Future<Output = Result<Location>> + Send + Sync;

        fn sim_info(
            &self,
            sim_id: &SimId,
        ) -> impl Future<Output = Result<SimInfo>> + Send + Sync;
    }
}

struct MockStatsd;

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
