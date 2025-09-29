use color_eyre::Result;
use nix::libc;
use orb_connd::{network_manager::NetworkManager, service::ConndService};
use orb_connd_dbus::ConndProxy;
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use std::{env, path::PathBuf, time::Duration};
use test_utils::docker::{self, Container};
use tokio::{task::JoinHandle, time};
use zbus::Address;

#[allow(dead_code)]
pub struct Fixture {
    pub nm: NetworkManager,
    container: Container,
    connd_server_handle: JoinHandle<Result<()>>,
    conn: zbus::Connection,
}

impl Fixture {
    pub async fn new(release: OrbRelease, platform: OrbOsPlatform) -> Self {
        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let docker_ctx = crate_dir.join("tests").join("docker");
        let dockerfile = crate_dir.join("tests").join("docker").join("Dockerfile");
        let tag = "worldcoin-nm";
        docker::build("worldcoin-nm", dockerfile, docker_ctx).await;

        let uid = unsafe { libc::geteuid() };
        let gid = unsafe { libc::getegid() };

        let container = docker::run(
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
        .await;

        time::sleep(Duration::from_secs(1)).await;

        let dbus_socket = container.tempdir.path().join("socket");
        let dbus_socket = format!("unix:path={}", dbus_socket.display());
        let addr: Address = dbus_socket.parse().unwrap();

        // todo: retry for
        let conn = zbus::ConnectionBuilder::address(addr)
            .unwrap()
            .build()
            .await
            .unwrap();

        let connd_server_handle =
            ConndService::new(conn.clone(), conn.clone(), release, platform).spawn();

        let secs = if env::var("GITHUB_ACTIONS").is_ok() {
            5
        } else {
            1
        };

        time::sleep(Duration::from_secs(secs)).await;

        Self {
            nm: NetworkManager::new(conn.clone()),
            conn,
            connd_server_handle,
            container,
        }
    }

    pub async fn connd(&self) -> ConndProxy<'_> {
        ConndProxy::new(&self.conn).await.unwrap()
    }
}
