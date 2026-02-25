#![allow(clippy::uninlined_format_args)]
use std::{path::PathBuf, pin::pin, time::Duration};

use bytes::Bytes;
use clap::Parser;
use color_eyre::{
    eyre::{bail, Context as _},
    Result,
};
use futures::{TryStream, TryStreamExt as _};
use humantime::parse_duration;
use orb_hil::{AuthMethod, RemoteConnectArgs, RemoteSession, RemoteTransport};
use secrecy::SecretString;
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt as _},
    sync::broadcast,
};
use tokio_serial::SerialPortBuilderExt as _;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, warn};

use crate::serial::{spawn_serial_reader_task, WaitErr};

const PATTERN_START: &str = "hil_pattern_start-";
const PATTERN_END: &str = "-hil_pattern_end";

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum CommandTransport {
    Serial,
    Ssh,
    Teleport,
}

impl CommandTransport {
    fn remote_transport(self) -> Option<RemoteTransport> {
        match self {
            CommandTransport::Serial => None,
            CommandTransport::Ssh => Some(RemoteTransport::Ssh),
            CommandTransport::Teleport => Some(RemoteTransport::Teleport),
        }
    }
}

#[derive(Debug, Parser)]
pub struct Cmd {
    /// Command to execute
    #[arg()]
    cmd: String,

    /// Transport used to run the command
    #[arg(long, value_enum, default_value_t = CommandTransport::Serial)]
    transport: CommandTransport,

    /// Path to the serial device
    #[arg(long, default_value = crate::serial::DEFAULT_SERIAL_PATH)]
    serial_path: PathBuf,

    /// Orb hostname for SSH, or teleport node ID for Teleport
    #[arg(long)]
    hostname: Option<String>,

    /// Orb ID used to derive a target when hostname is not provided
    #[arg(long)]
    orb_id: Option<String>,

    /// Username for SSH/Teleport
    #[arg(long)]
    username: Option<String>,

    /// SSH port (used only with --transport ssh)
    #[arg(long, default_value = "22")]
    port: u16,

    /// Password for SSH authentication (mutually exclusive with --key-path)
    #[arg(long)]
    password: Option<SecretString>,

    /// Path to SSH private key (mutually exclusive with --password)
    #[arg(long)]
    key_path: Option<PathBuf>,

    /// Timeout duration (e.g., "10s", "500ms")
    #[arg(long, default_value = "10s", value_parser = parse_duration)]
    timeout: Duration,
}

impl Cmd {
    pub async fn run(self) -> Result<()> {
        if let Some(remote_transport) = self.transport.remote_transport() {
            return self.run_remote(remote_transport).await;
        }

        self.run_serial().await
    }

    async fn run_serial(self) -> Result<()> {
        let serial = tokio_serial::new(
            self.serial_path.to_string_lossy(),
            crate::serial::ORB_BAUD_RATE,
        )
        .open_native_async()
        .wrap_err_with(|| {
            format!("failed to open serial port {}", self.serial_path.display())
        })?;
        let (serial_reader, serial_writer) = tokio::io::split(serial);

        run_inner(serial_reader, serial_writer, self.cmd, self.timeout).await
    }

    async fn run_remote(self, transport: RemoteTransport) -> Result<()> {
        let auth = self.resolve_remote_auth(transport)?;
        let connect_args = RemoteConnectArgs {
            transport,
            hostname: self.hostname,
            orb_id: self.orb_id,
            username: self.username,
            port: self.port,
            auth,
            timeout: self.timeout,
        };
        let session = RemoteSession::connect(connect_args).await?;

        let command_result =
            tokio::time::timeout(self.timeout, session.execute_command(&self.cmd))
                .await
                .wrap_err("remote command timed out")?
                .wrap_err("failed to execute remote command")?;

        print!("{}", command_result.stdout);
        eprint!("{}", command_result.stderr);
        if !command_result.is_success() {
            bail!(
                "command returned nonzero error code: {}",
                command_result.exit_status
            );
        }

        Ok(())
    }

    fn resolve_remote_auth(
        &self,
        transport: RemoteTransport,
    ) -> Result<Option<AuthMethod>> {
        match transport {
            RemoteTransport::Ssh => match (&self.password, &self.key_path) {
                (Some(password), None) => {
                    Ok(Some(AuthMethod::Password(password.clone())))
                }
                (None, Some(private_key_path)) => Ok(Some(AuthMethod::Key {
                    private_key_path: private_key_path.clone(),
                })),
                (None, None) => {
                    bail!("--transport ssh requires --password or --key-path")
                }
                (Some(_), Some(_)) => {
                    bail!("--password and --key-path are mutually exclusive")
                }
            },
            RemoteTransport::Teleport => {
                if self.password.is_some() || self.key_path.is_some() {
                    bail!(
                        "--password/--key-path can only be used with --transport ssh"
                    );
                }

                Ok(None)
            }
        }
    }
}

/// [`Cmd::run`], but the portion that is actually testable.
// TODO: actually test it >:)
async fn run_inner(
    serial_reader: impl AsyncRead + Send + 'static,
    serial_writer: impl AsyncWrite,
    cmd: String,
    timeout: Duration,
) -> Result<()> {
    let mut serial_writer = pin!(serial_writer);
    let (serial_tx, serial_rx) = broadcast::channel(64);
    let (reader_task, _kill_tx) = spawn_serial_reader_task(serial_reader, serial_tx);
    let mut serial_stream = BroadcastStream::new(serial_rx);

    let tty_fut = async {
        // Type newline to force a prompt (helps make sure we are in the state we
        // think we are in)
        type_str(&mut serial_writer, "\n").await?;
        wait_for_str(&mut serial_stream, "worldcoin@id", timeout)
            .await
            .wrap_err("failed while listening for prompt after newline")?;

        // Run cmd
        type_str(&mut serial_writer, &format!("stty -echo; {}\n\n", cmd)).await?;
        wait_for_str(&mut serial_stream, "worldcoin@id", timeout)
            .await
            .wrap_err("failed while listening for prompt after command")?;

        // Get command status code.
        type_str(
            &mut serial_writer,
            &format!("echo {PATTERN_START}$?{PATTERN_END}; stty echo\n"),
        )
        .await?;
        let extracted = extract_pattern(&mut serial_stream)
            .await
            .wrap_err("error while extracting pattern")?;
        let errcode: i32 = extracted
            .parse()
            .wrap_err("expected i32 from parsed string")?;
        debug!("got command error code: {errcode}");
        if errcode != 0 {
            bail!("command returned nonzero error code: {errcode}");
        }

        Ok(())
    };

    tokio::select! {
        result = tokio::time::timeout(timeout, tty_fut) => result.wrap_err("command timed out")?.wrap_err("error while executing command")?,
        result = reader_task => result.wrap_err("serial reader panicked")?.wrap_err("error in serial reader task")?,
    }

    Ok(())
}

/// Extracts `pattern` from `serial_stream`. Returns `None` if the stream terminated.
// TODO: Write tests
async fn extract_pattern<E>(
    mut serial_stream: impl TryStream<Ok = Bytes, Error = E> + Unpin,
) -> Result<String, WaitErr<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    let mut buf = String::with_capacity(64);
    loop {
        let Some(chunk) = serial_stream.try_next().await? else {
            break;
        };

        let Ok(str) = std::str::from_utf8(&chunk) else {
            warn!("encountered non-utf8 data, dropping it");
            continue;
        };
        buf.push_str(str);
        // TODO(@thebutlah): We can advance the slice checked to make it more efficient.
        // but before I implement that, I would want tests.
        if let Some(extracted) = extract_pattern_no_io(&buf) {
            return Ok(extracted.to_string());
        }
    }
    debug!("serial stream ended, terminating future");

    Err(WaitErr::StreamEnded)
}

/// Extracts the str sandwiched between [`PATTERN_START`] and [`PATTERN_END`].
fn extract_pattern_no_io(s: &str) -> Option<&str> {
    Some(s.split_once(PATTERN_START)?.1.split_once(PATTERN_END)?.0)
}

/// Types out the string `s` into `serial_writer`.
async fn type_str(mut serial_writer: impl AsyncWrite + Unpin, s: &str) -> Result<()> {
    serial_writer
        .write_all(s.as_bytes())
        .await
        .wrap_err_with(|| format!("failed to type {s}"))
}

/// Returns when `pattern` is detected in the `serial_stream`.
///
/// Includes timeouts.
async fn wait_for_str<E>(
    serial_stream: impl TryStream<Ok = Bytes, Error = E>,
    pattern: &str,
    timeout: Duration,
) -> Result<()>
where
    E: std::error::Error + Send + Sync + 'static,
{
    tokio::time::timeout(
        timeout,
        crate::serial::wait_for_pattern(pattern.as_bytes().to_vec(), serial_stream),
    )
    .await
    .wrap_err_with(|| format!("timeout while waiting for {pattern}"))?
    .wrap_err_with(|| format!("error while waiting for {pattern}"))
}

#[cfg(test)]
mod test {
    use super::*;

    fn sample_cmd() -> Cmd {
        Cmd {
            cmd: "pwd".to_owned(),
            transport: CommandTransport::Ssh,
            serial_path: PathBuf::from("/dev/null"),
            hostname: Some("orb-test.local".to_owned()),
            orb_id: None,
            username: None,
            port: 22,
            password: None,
            key_path: None,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn ssh_transport_requires_auth() {
        let cmd = sample_cmd();
        let err = cmd
            .resolve_remote_auth(RemoteTransport::Ssh)
            .expect_err("ssh must require auth");
        assert!(err
            .to_string()
            .contains("--transport ssh requires --password or --key-path"));
    }

    #[test]
    fn ssh_transport_accepts_password_auth() {
        let mut cmd = sample_cmd();
        cmd.password = Some(SecretString::from("password".to_owned()));

        let auth = cmd
            .resolve_remote_auth(RemoteTransport::Ssh)
            .expect("password auth should be accepted");
        assert!(matches!(auth, Some(AuthMethod::Password(_))));
    }

    #[test]
    fn ssh_transport_rejects_both_auth_methods() {
        let mut cmd = sample_cmd();
        cmd.password = Some(SecretString::from("password".to_owned()));
        cmd.key_path = Some(PathBuf::from("/tmp/id_rsa"));

        let err = cmd
            .resolve_remote_auth(RemoteTransport::Ssh)
            .expect_err("ssh must reject dual auth methods");
        assert!(err
            .to_string()
            .contains("--password and --key-path are mutually exclusive"));
    }

    #[test]
    fn teleport_transport_rejects_ssh_auth_flags() {
        let mut cmd = sample_cmd();
        cmd.password = Some(SecretString::from("password".to_owned()));

        let err = cmd
            .resolve_remote_auth(RemoteTransport::Teleport)
            .expect_err("teleport must reject ssh auth flags");
        assert!(err
            .to_string()
            .contains("--password/--key-path can only be used with --transport ssh"));
    }
}
