use nix::sys::socket::{bind, socket, AddressFamily, SockFlag, SockType, UnixAddr};
use std::os::fd::IntoRawFd;
use std::path::PathBuf;
use std::process::Output;
use std::{env, io};
use tempfile::TempDir;
use testcontainers::core::Mount;
use testcontainers::runners::AsyncRunner;
use testcontainers::{core::WaitFor, ContainerAsync, GenericImage, ImageExt};

pub const DBUS_IMAG_TAG: &str = "worldcoin-debian-dbus";

/// Builds an image with the `DBUS_IMG_TAG` tag.
pub async fn build_dbus_img() -> io::Result<Output> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let output = tokio::process::Command::new("docker")
        .arg("build")
        .arg("-t")
        .arg(DBUS_IMAG_TAG)
        .arg("-f")
        .arg(crate_dir.join("Dockerfile"))
        .arg(crate_dir)
        .output()
        .await?;

    Ok(output)
}

/// An abstraction over a container with its own d-bus instance running.
/// Creates a temporary directory with a socket used to communicate with d-bus.
/// Once it goes out of scope, container running dbus is stopped and temporary dir with socket is
/// cleaned up.
pub struct DbusContainer {
    tempdir: TempDir,
    _container: ContainerAsync<GenericImage>,
}

impl DbusContainer {
    pub async fn new() -> Self {
        build_dbus_img().await.unwrap();
        let tempdir = TempDir::new_in("/tmp").unwrap();
        let tempdir_path = tempdir.path().canonicalize().unwrap();

        let sock_fd = socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::empty(),
            None,
        )
        .unwrap()
        .into_raw_fd();

        let sockaddr = UnixAddr::new(&tempdir_path.join("socket")).unwrap();
        bind(sock_fd, &sockaddr).unwrap();

        let _container = GenericImage::new(DBUS_IMAG_TAG, "latest")
            .with_wait_for(WaitFor::Nothing)
            .with_mount(Mount::bind_mount(
                tempdir_path.to_string_lossy().into_owned(),
                "/run/integration-tests",
            ))
            .start()
            .await
            .unwrap();

        Self {
            _container,
            tempdir,
        }
    }

    /// Returns the path to the temporary socket file
    pub fn socket_path(&self) -> PathBuf {
        self.tempdir.path().join("socket")
    }

    /// Returns a formatted d-bus session address
    /// e.g.: `unix:path=/tmp/jKds12Nd/socket`
    pub fn dbus_session_address(&self) -> String {
        let socket_path = self.socket_path().to_string_lossy().into_owned();
        format!("unix:path={socket_path}")
    }

    pub fn set_host_dbus_session_address(&self) {
        // This operation requires test to be run serially.
        env::set_var("DBUS_SESSION_BUS_ADDRESS", self.dbus_session_address());
    }
}
