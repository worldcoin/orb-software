#![allow(clippy::uninlined_format_args)]
use std::pin::pin;

use bytes::Bytes;
use color_eyre::{eyre::Context as _, Result};
use futures::{TryStream, TryStreamExt};
use tokio::{
    io::{AsyncRead, AsyncWriteExt as _},
    sync::{
        broadcast::{self, error::SendError},
        oneshot,
    },
    task::JoinHandle,
};
use tokio_util::io::ReaderStream;
use tracing::debug;

use self::stream_processing::{SerialLogEvent, SerialProcessor};

mod stream_processing;

pub const LOGIN_PROMPT_PATTERN: &str = "login:";
const KERNEL_PANIC_PATERN: &str = "Kernel panic:";
pub const ORB_BAUD_RATE: u32 = 115200;
pub const DEFAULT_SERIAL_PATH: &str = if cfg!(target_os = "linux") {
    "/dev/ttyUSB0"
} else {
    "TODO"
};

/// Spawns a task that pushes serial data into `serial_output_tx`.
// TODO: Write tests for this
pub fn spawn_serial_reader_task(
    reader: impl AsyncRead + Send + 'static,
    serial_output_tx: broadcast::Sender<Bytes>,
) -> (JoinHandle<Result<()>>, oneshot::Sender<()>) {
    let (kill_tx, mut kill_rx) = oneshot::channel();
    let reader_task = tokio::task::spawn(async move {
        let mut serial_stream = pin!(ReaderStream::new(reader));
        let mut stderr = tokio::io::stderr();
        let mut line_buf: Vec<u8> = Vec::new();
        loop {
            let chunk = tokio::select! {
                _ = &mut kill_rx => break,
                chunk = serial_stream.try_next() => chunk,
            };
            let Some(chunk) = chunk.wrap_err("failed to read from serial")? else {
                break;
            };
            for &byte in chunk.iter() {
                if byte == b'\n' {
                    line_buf.push(byte);
                    if !is_progress_line(&line_buf) {
                        let _ = stderr.write_all(&line_buf).await;
                    }
                    line_buf.clear();
                } else {
                    line_buf.push(byte);
                }
            }
            if let Err(SendError(_)) = serial_output_tx.send(chunk) {
                break;
            }
        }
        if !line_buf.is_empty() && !is_progress_line(&line_buf) {
            let _ = stderr.write_all(&line_buf).await;
        }
        debug!("terminating serial task");
        Ok::<(), color_eyre::Report>(())
    });

    (reader_task, kill_tx)
}

/// Returns true for progress bar lines like `[ 515.3065 ] [.....] 100%`,
/// which should be suppressed from stderr output.
fn is_progress_line(line: &[u8]) -> bool {
    line.iter()
        .rev()
        .find(|b| !b.is_ascii_whitespace())
        .map(|&b| b == b'%')
        .unwrap_or(false)
}

#[derive(thiserror::Error, Debug)]
pub enum WaitErr<E> {
    #[error("stream ended without finding the pattern")]
    StreamEnded,
    #[error(transparent)]
    Other(#[from] E),
}

/// Completes when the login prompt has been reached.
pub async fn wait_for_pattern<B, E>(
    pattern: Vec<u8>,
    serial_stream: impl TryStream<Ok = B, Error = E>,
) -> Result<(), WaitErr<E>>
where
    B: AsRef<[u8]>,
{
    let log_events = SerialProcessor::new(pattern)
        .listen_for_events(serial_stream)
        .try_filter(|evt| {
            std::future::ready(matches!(evt, SerialLogEvent::PatternFound))
        });
    let mut log_events = pin!(log_events);
    if let Some(SerialLogEvent::PatternFound) = log_events.try_next().await? {
        return Ok(());
    }

    Err(WaitErr::StreamEnded)
}

#[cfg(test)]
mod test {
    use super::*;

    async fn assert_has_prompt(text: &str) {
        let fake_serial = std::io::Cursor::new(text);
        let mut stream = ReaderStream::new(fake_serial);
        assert!(
            wait_for_pattern(LOGIN_PROMPT_PATTERN.to_owned().into_bytes(), &mut stream)
                .await
                .is_ok(),
            "text contained the login prompt, so this should have worked"
        );
        assert!(
            matches!(
                wait_for_pattern(
                    LOGIN_PROMPT_PATTERN.to_owned().into_bytes(),
                    &mut stream
                )
                .await,
                Err(WaitErr::StreamEnded)
            ),
            "already processed all the text, so awaiting again *should* end the stream"
        );
    }

    #[tokio::test]
    async fn test_multiline() {
        let text = "asdfasbgasdoi
            \0\0afoobarbaz


            \n\r yeetuslocalhost login:spammytext";
        assert_has_prompt(text).await
    }

    #[tokio::test]
    async fn test_exact() {
        let text = LOGIN_PROMPT_PATTERN;
        assert_has_prompt(text).await
    }

    #[tokio::test]
    async fn test_repeat() {
        let text = LOGIN_PROMPT_PATTERN.repeat(4);
        assert_has_prompt(&text).await
    }
}
