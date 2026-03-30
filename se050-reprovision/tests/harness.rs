use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};

use color_eyre::{
    eyre::{bail, eyre, Context},
    Result,
};
use rand::SeedableRng;
use std::os::unix::net::{UnixListener, UnixStream};
use tempfile::TempDir;

#[derive(Debug, bon::Builder)]
pub struct Harness {
    #[builder(default = TempDir::new().expect("failed to create tempdir"))]
    tempdir: tempfile::TempDir,
    seed: u64,
    #[builder(default = Duration::from_millis(5000))]
    timeout: Duration,
    #[builder(skip = spawn_accept(tempdir.path().join("unix_socket"), timeout))]
    proxied_io: flume::Receiver<Result<UnixStream>>,
    #[builder(skip = build_cli_stub())]
    built_cli_stub_path: PathBuf,
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
}

fn build_cli_stub() -> PathBuf {
    todo!()
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
