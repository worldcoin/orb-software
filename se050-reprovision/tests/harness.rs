use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};

use color_eyre::{
    eyre::{bail, ensure, eyre, Context},
    Result,
};
use rand::SeedableRng;
use std::os::unix::net::{UnixListener, UnixStream};
use tempfile::TempDir;
use tracing::info;

const SOCKET_NAME: &str = "cli_stub.sock";

#[derive(Debug, bon::Builder)]
pub struct Harness {
    #[builder(default = TempDir::new().expect("failed to create tempdir"))]
    tempdir: tempfile::TempDir,
    seed: u64,
    #[builder(default = Duration::from_millis(5000))]
    timeout: Duration,
    #[builder(skip = build_cli_stub(&tempdir.path().join("cargo")).expect("failed to build stub binary"))]
    built_cli_stub_path: PathBuf,
    #[builder(skip = Some(spawn_accept(tempdir.path().join(SOCKET_NAME), timeout)))]
    proxied_io: Option<flume::Receiver<Result<UnixStream>>>,
    mocked_server: wiremock::MockServer,
}

impl Harness {
    pub fn make_program_cfg(&self) -> orb_se050_reprovision::Config {
        orb_se050_reprovision::Config {
            rng: rand::rngs::StdRng::seed_from_u64(self.seed),
            client: orb_se050_reprovision::remote_api::Client::builder()
                .local_backend(self.mocked_server.address().port())
                .custom_reqwest_client(reqwest::Client::new())
                .build(),
            ca_path: self.built_cli_stub_path.clone(),
        }
    }

    /// Panics if called more than once
    pub fn take_stream(&mut self) -> flume::Receiver<Result<UnixStream>> {
        self.proxied_io
            .take()
            .expect("already got stream, don't call this function more than once")
    }
}

fn build_cli_stub(out_dir: &Path) -> Result<PathBuf> {
    let exit_status = escargot::CargoBuild::new()
        .example("stubbed_binary")
        .manifest_path(env!("CARGO_MANIFEST_PATH"))
        .target_dir(out_dir)
        .into_command()
        .status()
        .wrap_err("failed to build stubbed cargo binary")?;
    ensure!(exit_status.success(), "nonzero exit code");

    // TODO: Is there a better way to get the output path from the build messages
    Ok(out_dir
        .join("debug")
        .join("examples")
        .join("stubbed_binary"))
}

fn spawn_accept(
    path: PathBuf,
    accept_timeout: Duration,
) -> flume::Receiver<Result<UnixStream>> {
    let (tx, rx) = flume::unbounded();
    std::thread::spawn(move || {
        let result = listen_and_accept(&path, accept_timeout);
        let _ = tx.send(result);
    });

    rx
}

fn listen_and_accept(path: &Path, accept_timeout: Duration) -> Result<UnixStream> {
    info!("binding unix listener at {}", path.display());
    let listener = UnixListener::bind(path).wrap_err_with(|| {
        format!("failed to bind unix listener at {}", path.display())
    })?;
    listener.set_nonblocking(true)?;

    let start_time = std::time::Instant::now();
    let mut accept_result = Err(eyre!(
        "timed out while waiting for a unix socket to connect"
    ));
    while start_time.elapsed() < accept_timeout {
        match listener.accept() {
            Ok((stream, _addr)) => accept_result = Ok(stream),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(err) => {
                accept_result =
                    Err(err).wrap_err("error while listening for connection");
                break;
            }
        }
    }
    let stream = accept_result?;
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(accept_timeout))?;
    stream.set_write_timeout(Some(accept_timeout))?;

    Ok(stream)
}
