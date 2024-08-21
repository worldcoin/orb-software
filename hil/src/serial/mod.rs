use std::pin::pin;

use futures::{TryStream, TryStreamExt};

use self::stream_processing::{SerialLogEvent, SerialProcessor};

mod stream_processing;

const LOGIN_PROMPT: &str = "localhost login:";
const KERNEL_PANIC_PATERN: &str = "Kernel panic:";

#[derive(thiserror::Error, Debug)]
pub enum WaitErr<E> {
    #[error(transparent)]
    Other(#[from] E),
    #[error("stream ended without finding the login prompt")]
    StreamEnded,
}

/// Completes when the login prompt has been reached.
pub async fn wait_for_login_prompt<B, E>(
    serial_stream: impl TryStream<Ok = B, Error = E>,
) -> Result<(), WaitErr<E>>
where
    B: AsRef<[u8]>,
{
    let mut log_events = SerialProcessor::new()
        .listen_for_events(serial_stream)
        .try_filter(|evt| {
            std::future::ready(matches!(evt, SerialLogEvent::LoginPrompt))
        });
    let mut log_events = pin!(log_events);
    if let Some(SerialLogEvent::LoginPrompt) = log_events.try_next().await? {
        return Ok(());
    }

    Err(WaitErr::StreamEnded)
}

#[cfg(test)]
mod test {
    use super::*;

    async fn assert_has_prompt(text: &str) {
        let fake_serial = std::io::Cursor::new(text);
        let mut stream = tokio_util::io::ReaderStream::new(fake_serial);
        assert!(
            wait_for_login_prompt(&mut stream).await.is_ok(),
            "text contained the login prompt, so this should have worked"
        );
        assert!(
            matches!(
                wait_for_login_prompt(&mut stream).await,
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
        let text = LOGIN_PROMPT;
        assert_has_prompt(text).await
    }

    #[tokio::test]
    async fn test_repeat() {
        let text = LOGIN_PROMPT.repeat(4);
        assert_has_prompt(&text).await
    }
}
