#![cfg(feature = "testing")]
#![allow(dead_code)]

//! Provides isolated infrastructure for monitoring-auth integration tests.
//!
//! Tests supply behavioral dependencies such as `Clock` and `SecureStorage`.
//! The fixture owns external infrastructure, application startup, readiness,
//! client invocation, and cleanup.

use async_tempfile::TempDir;
use chrono::{DateTime, Utc};
use color_eyre::Result;
use dbus_launch::BusType;
use orb_info::OrbId;
use orb_monitoring_auth::{
    server::{self, secure_storage::SecureStorage, Clock, Dependencies},
    Token,
};
use std::{
    io::ErrorKind,
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use tokio::{task::JoinHandle, time};
use wiremock::MockServer;

/// Constructs a token through its serialized production contract.
pub fn token(value: &str, expiry: DateTime<Utc>) -> Token {
    serde_json::from_value(serde_json::json!({
        "token": value,
        "expiry": expiry.timestamp_millis(),
    }))
    .expect("fixture token should deserialize")
}

/// Prepared, isolated infrastructure that has not started the server.
pub struct Fixture {
    tempdir: TempDir,
    dbusd: dbus_launch::Daemon,
    dbus: zbus::Connection,
    socket_path: PathBuf,
    pub backend: MockServer,
    clock: Clock,
    secure_storage: SecureStorage,
    orb_id: OrbId,
    uid: u32,
}

impl Fixture {
    /// Prepares isolated infrastructure around the supplied server dependencies.
    /// It does not start the monitoring-auth server.
    pub async fn new(clock: Clock, secure_storage: SecureStorage) -> Self {
        let tempdir = TempDir::new()
            .await
            .expect("failed to create monitoring-auth test directory");
        let socket_parent = tempdir.to_path_buf().join("server");
        tokio::fs::create_dir_all(&socket_parent)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "failed to create socket directory at {}: {error}",
                    socket_parent.display()
                )
            });
        let socket_path = socket_parent.join("socket");

        let dbusd = tokio::task::spawn_blocking(|| {
            dbus_launch::Launcher::daemon()
                .bus_type(BusType::Session)
                .launch()
                .expect("failed to launch test D-Bus daemon")
        })
        .await
        .expect("D-Bus launcher task panicked");
        let dbus = zbus::ConnectionBuilder::address(dbusd.address())
            .expect("test D-Bus daemon returned an invalid address")
            .build()
            .await
            .expect("failed to connect to test D-Bus daemon");

        let backend = MockServer::start().await;
        let orb_id = OrbId::from_str("bba85baa").expect("test Orb ID should parse");
        let uid = unsafe { nix::libc::getuid() };

        Self {
            tempdir,
            dbusd,
            dbus,
            socket_path,
            backend,
            clock,
            secure_storage,
            orb_id,
            uid,
        }
    }

    /// Starts the production server and waits until its Unix socket is ready.
    pub async fn run(self) -> FxHandle {
        let Self {
            tempdir,
            dbusd,
            dbus,
            socket_path,
            backend,
            clock,
            secure_storage,
            orb_id,
            uid,
        } = self;

        let dependencies = Dependencies {
            token_endpoint: format!("{}/monitoring-token", backend.uri()),
            server_socket_path: socket_path.clone(),
            dd_agent_uid: uid,
            clock,
            dbus,
            refresh_token_interval: Duration::from_hours(4),
            orb_id,
            secure_storage,
        };
        let mut server = tokio::spawn(server::main(dependencies));

        wait_for_socket(&socket_path, &mut server).await;

        FxHandle {
            server,
            socket_path,
            backend,
            uid,
            _tempdir: tempdir,
            _dbusd: dbusd,
        }
    }
}

/// A running monitoring-auth server and its fixture-owned infrastructure.
pub struct FxHandle {
    server: JoinHandle<Result<()>>,
    socket_path: PathBuf,
    pub backend: MockServer,
    uid: u32,
    _tempdir: TempDir,
    _dbusd: dbus_launch::Daemon,
}

impl FxHandle {
    /// Runs the real client and returns the bytes written to its output.
    pub async fn run_client(&self) -> Result<Vec<u8>> {
        let uid = self.uid;
        let socket_path = self.socket_path.clone();

        tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let mut output = Vec::new();
            orb_monitoring_auth::client::main(uid, socket_path, &mut output)?;

            Ok(output)
        })
        .await
        .expect("client task panicked")
    }

    /// Aborts and joins the server task before releasing fixture resources.
    pub async fn stop(mut self) {
        self.server.abort();
        let _ = (&mut self.server).await;
    }
}

impl Drop for FxHandle {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn wait_for_socket(socket_path: &Path, server: &mut JoinHandle<Result<()>>) {
    let readiness = async {
        loop {
            if server.is_finished() {
                match server.await {
                    Ok(Ok(())) => panic!(
                        "server exited before creating socket at {}",
                        socket_path.display()
                    ),
                    Ok(Err(error)) => panic!(
                        "server exited before creating socket at {}: {error:?}",
                        socket_path.display()
                    ),
                    Err(error) => panic!(
                        "server task failed before creating socket at {}: {error}",
                        socket_path.display()
                    ),
                }
            }

            match tokio::fs::metadata(socket_path).await {
                Ok(metadata) if metadata.file_type().is_socket() => return,
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => panic!(
                    "failed to inspect server socket at {}: {error}",
                    socket_path.display()
                ),
            }

            time::sleep(Duration::from_millis(10)).await;
        }
    };

    time::timeout(Duration::from_secs(5), readiness)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "server did not create socket at {} before timeout",
                socket_path.display()
            )
        });
}
