#![allow(clippy::uninlined_format_args)]
use std::{pin::pin, time::Duration};

use crate::{RemoteArgs, RemoteTransport};
use bytes::Bytes;
use clap::Parser;
use color_eyre::{
    eyre::{bail, Context as _},
    Result,
};
use futures::{TryStream, TryStreamExt as _};
use humantime::parse_duration;
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt as _},
    sync::broadcast,
};
use tokio_serial::SerialPortBuilderExt as _;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, warn};

use crate::serial::{spawn_serial_reader_task, WaitErr};
use crate::OrbConfig;

const PATTERN_START: &str = "hil_pattern_start-";
const PATTERN_END: &str = "-hil_pattern_end";
const SHELL_PROMPT_PATTERNS: &[&str] = &["worldcoin@id", "root@"];

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

    /// Timeout duration (e.g., "10s", "500ms")
    #[arg(long, default_value = "10s", value_parser = parse_duration)]
    timeout: Duration,

    #[command(flatten)]
    remote: RemoteArgs,
}

impl Cmd {
    pub async fn run(self, orb_config: &OrbConfig) -> Result<()> {
        if let Some(remote_transport) = self.transport.remote_transport() {
            return self.run_remote(remote_transport, orb_config).await;
        }

        self.run_serial(orb_config).await
    }

    async fn run_serial(self, orb_config: &OrbConfig) -> Result<()> {
        let serial_path = if let Some(custom_path) = orb_config.serial_path.as_ref() {
            custom_path.as_path()
        } else {
            std::path::Path::new(crate::serial::DEFAULT_SERIAL_PATH)
        };

        let serial = tokio_serial::new(
            serial_path.to_string_lossy(),
            crate::serial::ORB_BAUD_RATE,
        )
        .open_native_async()
        .wrap_err_with(|| {
            format!("failed to open serial port {}", serial_path.display())
        })?;
        let (serial_reader, serial_writer) = tokio::io::split(serial);

        run_inner(serial_reader, serial_writer, self.cmd, self.timeout).await
    }

    async fn run_remote(
        self,
        transport: RemoteTransport,
        orb_config: &OrbConfig,
    ) -> Result<()> {
        let session = self
            .remote
            .connect(transport, self.timeout, orb_config)
            .await?;

        let command_result =
            tokio::time::timeout(self.timeout, session.execute_command(&self.cmd))
                .await
                .wrap_err("remote command timed out")?
                .wrap_err("failed to execute remote command")?;

        print!("{}", command_result.stdout);
        eprint!("{}", command_result.stderr);
        if !command_result.is_success() {
            color_eyre::eyre::bail!(
                "command returned nonzero error code: {}",
                command_result.exit_status
            );
        }

        Ok(())
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
        wait_for_prompt(&mut serial_stream, timeout)
            .await
            .wrap_err("failed while listening for prompt after newline")?;

        // Run cmd
        type_str(&mut serial_writer, &format!("stty -echo; {}\n\n", cmd)).await?;
        wait_for_prompt(&mut serial_stream, timeout)
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

/// Returns when a shell prompt is detected in the `serial_stream`.
///
/// Includes timeouts.
async fn wait_for_prompt<E>(
    serial_stream: impl TryStream<Ok = Bytes, Error = E> + Unpin,
    timeout: Duration,
) -> Result<()>
where
    E: std::error::Error + Send + Sync + 'static,
{
    let patterns = SHELL_PROMPT_PATTERNS.join(", ");
    tokio::time::timeout(
        timeout,
        wait_for_any_pattern(serial_stream, SHELL_PROMPT_PATTERNS),
    )
    .await
    .wrap_err_with(|| format!("timeout while waiting for one of {patterns}"))?
    .map(|matched| {
        debug!("detected shell prompt pattern: {matched:?}");
    })
    .wrap_err_with(|| format!("error while waiting for one of {patterns}"))
}

async fn wait_for_any_pattern<'a, E>(
    mut serial_stream: impl TryStream<Ok = Bytes, Error = E> + Unpin,
    patterns: &'a [&'a str],
) -> Result<&'a str, WaitErr<E>> {
    let max_pattern_len = patterns
        .iter()
        .map(|pattern| pattern.len())
        .max()
        .expect("at least one prompt pattern");
    let mut buf = Vec::with_capacity(max_pattern_len * 2);

    loop {
        let Some(chunk) = serial_stream.try_next().await? else {
            break;
        };

        buf.extend_from_slice(&chunk);
        if let Some(pattern) = find_matching_pattern(&buf, patterns) {
            return Ok(pattern);
        }

        let keep_len = max_pattern_len.saturating_sub(1);
        if buf.len() > keep_len {
            buf.drain(..buf.len() - keep_len);
        }
    }

    Err(WaitErr::StreamEnded)
}

fn find_matching_pattern<'a>(buf: &[u8], patterns: &'a [&'a str]) -> Option<&'a str> {
    patterns
        .iter()
        .find(|pattern| {
            buf.windows(pattern.len())
                .any(|window| window == pattern.as_bytes())
        })
        .copied()
}

#[cfg(test)]
mod test {
    use std::convert::Infallible;

    use super::*;

    fn sample_cmd() -> Cmd {
        Cmd {
            cmd: "pwd".to_owned(),
            transport: CommandTransport::Ssh,
            timeout: Duration::from_secs(5),
            remote: RemoteArgs {
                hostname: None,
                username: None,
                port: 22,
                password: None,
                key_path: None,
            },
        }
    }

    #[test]
    fn serial_transport_has_no_remote_transport() {
        let cmd = Cmd {
            transport: CommandTransport::Serial,
            ..sample_cmd()
        };
        assert!(cmd.transport.remote_transport().is_none());
    }

    #[tokio::test]
    async fn wait_for_any_pattern_detects_worldcoin_prompt() {
        let stream = futures::stream::iter(
            [Bytes::from_static(b"abc worldcoin@id-123:~$")]
                .into_iter()
                .map(Ok::<_, Infallible>),
        );

        assert_eq!(
            wait_for_any_pattern(stream, SHELL_PROMPT_PATTERNS)
                .await
                .expect("prompt should be detected"),
            "worldcoin@id"
        );
    }

    #[tokio::test]
    async fn wait_for_any_pattern_detects_root_prompt_split_across_chunks() {
        let stream = futures::stream::iter(
            [
                Bytes::from_static(b"boot noise ro"),
                Bytes::from_static(b"ot@id-123:~#"),
            ]
            .into_iter()
            .map(Ok::<_, Infallible>),
        );

        assert_eq!(
            wait_for_any_pattern(stream, SHELL_PROMPT_PATTERNS)
                .await
                .expect("prompt should be detected"),
            "root@"
        );
    }

    #[tokio::test]
    async fn wait_for_any_pattern_returns_stream_ended_without_prompt() {
        let stream = futures::stream::iter(
            [Bytes::from_static(b"boot noise")]
                .into_iter()
                .map(Ok::<_, Infallible>),
        );

        assert!(matches!(
            wait_for_any_pattern(stream, SHELL_PROMPT_PATTERNS).await,
            Err(WaitErr::StreamEnded)
        ));
    }
}
