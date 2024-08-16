use std::{
    pin::pin,
    sync::atomic::{AtomicBool, Ordering},
};

use futures::{Stream, StreamExt as _};

use self::stream_processing::listen_for_pattern;

mod stream_processing;

const LOGIN_PROMPT: &str = "localhost login:";

#[derive(thiserror::Error, Debug, Eq, PartialEq)]
#[error("stream ended without finding the login prompt")]
pub struct StreamEnded;

/// Completes when the login prompt has been reached.
pub async fn wait_for_login_prompt<B>(
    serial_stream: impl Stream<Item = B>,
) -> Result<(), StreamEnded>
where
    B: AsRef<[u8]>,
{
    let found = AtomicBool::new(false);
    let stream = listen_for_pattern(LOGIN_PROMPT, serial_stream, || {
        println!("found match!");
        found.store(true, Ordering::SeqCst);
    });

    let mut stream = pin!(stream);
    while let Some(_bytes) = stream.next().await {
        if found.load(Ordering::SeqCst) {
            return Ok(());
        }
    }
    Err(StreamEnded)
}

#[cfg(test)]
mod test {
    use super::*;

    async fn assert_has_prompt(text: &str) {
        let fake_serial = std::io::Cursor::new(text);
        let mut stream = tokio_util::io::ReaderStream::new(fake_serial)
            .map(|result| result.expect("cursor will never give io error"));
        assert!(
            wait_for_login_prompt(&mut stream).await.is_ok(),
            "text contained the login prompt, so this should have worked"
        );
        assert!(
            matches!(wait_for_login_prompt(&mut stream).await, Err(StreamEnded)),
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
