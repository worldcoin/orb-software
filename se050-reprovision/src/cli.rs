use std::{
    array::TryFromSliceError,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use bytes::Bytes;
use color_eyre::eyre::{Context, Result};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::io::{CopyToBytes, SinkWriter, StreamReader};

const CLI_TIMEOUT: Duration = Duration::from_millis(10_000);

#[derive(Debug, Serialize, Deserialize)]
pub struct CliOutput {
    pub jetson_authkey: KeyInfo,
    pub attestation_key: KeyInfo,
    pub iris_code_key: KeyInfo,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyInfo {
    /// PEM format
    pub key: String,
    #[serde(with = "crate::base64_serde")]
    pub signature: Vec<u8>,
    #[serde(with = "crate::base64_serde")]
    pub extra_data: Vec<u8>,
    // active: bool,
}

struct Child {
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
}

#[derive(Debug)]
pub struct MockChild {
    pub stdin: flume::Sender<Bytes>,
    pub stdout: flume::Receiver<Bytes>,
}

async fn call_cli(path: &Path) -> Result<Child> {
    let mut child = tokio::process::Command::new(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err_with(|| format!("failed to spawn {}", path.display()))?;
    let stdin = child.stdin.take().expect("infallible");
    let stdout = child.stdout.take().expect("infallible");

    Ok(Child { stdin, stdout })
}

#[derive(Debug)]
pub enum CliStrategy {
    Process(PathBuf),
    Mocked(MockChild),
}

#[derive(Default, derive_more::From)]
pub struct Nonce(pub [u8; 16]);

impl TryFrom<&[u8]> for Nonce {
    type Error = TryFromSliceError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(value.try_into()?))
    }
}

impl rand::Fill for Nonce {
    fn try_fill<R: rand::Rng + ?Sized>(
        &mut self,
        rng: &mut R,
    ) -> Result<(), rand::Error> {
        Ok(rng.fill_bytes(&mut self.0))
    }
}

/// Mockable cli calling.
pub async fn call(cfg: CliStrategy, nonce: Nonce) -> Result<CliOutput> {
    let output = tokio::time::timeout(CLI_TIMEOUT, call_bytes(cfg, &nonce.0))
        .await
        .wrap_err("timed out while calling cli")?
        .wrap_err("error while calling cli")?;

    serde_json::from_str(&output).wrap_err("failed to deserialize CLI output")
}

/// Low level, bytes-oriented cli call
async fn call_bytes(strategy: CliStrategy, bytes: &[u8]) -> Result<String> {
    let (mut stdin, mut stdout): (
        Box<dyn AsyncWrite + Unpin + Send + Sync>,
        Box<dyn AsyncRead + Unpin + Send + Sync>,
    ) = match strategy {
        CliStrategy::Process(path) => {
            let Child { stdin, stdout } = call_cli(&path).await?;
            (Box::new(stdin), Box::new(stdout))
        }
        CliStrategy::Mocked(MockChild { stdin, stdout }) => {
            let stdin = SinkWriter::new(CopyToBytes::new(
                stdin.into_sink().sink_map_err(|err @ flume::SendError(_)| {
                    std::io::Error::new(ErrorKind::BrokenPipe, err)
                }),
            ));
            let stdout =
                StreamReader::new(stdout.into_stream().map(Ok::<_, std::io::Error>));

            (Box::new(stdin), Box::new(stdout))
        }
    };

    stdin
        .write_all(bytes)
        .await
        .wrap_err("failed to write nonce to stdin")?;
    stdin
        .shutdown()
        .await
        .wrap_err("failed to shutdown stdin")?;
    drop(stdin);

    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .await
        .wrap_err("failed to read from stdout")?;

    Ok(output)
}

#[cfg(test)]
mod test {
    use std::{io::Write as _, os::unix::fs::PermissionsExt as _};

    use tempfile::TempDir;
    use tracing::info;

    use super::*;

    #[test]
    fn test_subprocess_call() -> Result<()> {
        let _ = color_eyre::install();
        let _ = orb_telemetry::TelemetryConfig::new().init();

        let dummy_cli = r#"#!/usr/bin/env sh
            cat /dev/stdin
        "#;

        // set up cli
        let tmp = TempDir::new()?;
        let cli_path = tmp.path().join("cli");
        let mut cli = std::fs::File::create_new(&cli_path)?;
        cli.set_permissions(std::fs::Permissions::from_mode(0o500))?;
        let actual_perms = std::fs::metadata(&cli_path)?.permissions();
        assert_eq!(
            actual_perms.mode() & 0o500,
            0o500,
            "wasn't able to get an executable tempfile"
        );
        cli.write_all(dummy_cli.as_bytes())?;
        cli.sync_all()?;
        drop(cli);
        info!("cli path: {cli_path:?}");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let _rt_guard = rt.enter();
        let call_fut = tokio::time::timeout(
            Duration::from_millis(500),
            call_bytes(
                CliStrategy::Process(cli_path.to_owned()),
                "foobar".as_bytes(),
            ),
        );
        let output = rt
            .block_on(call_fut)
            .wrap_err("timeout")?
            .wrap_err("failed to call cli")?;

        assert_eq!(output, "foobar");

        Ok(())
    }
}
