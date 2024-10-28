use color_eyre::eyre::Result;
use orb_sound::Queue;
use std::{fs, io::Cursor, sync::Arc, time::Duration};
use tokio::time::sleep;

#[derive(Clone)]
struct Sound(Arc<Vec<u8>>);

impl AsRef<[u8]> for Sound {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let queue = Queue::spawn("default")?;

    let connected = Sound(Arc::new(fs::read("sound/assets/voice_connected.wav")?));
    let timeout = Sound(Arc::new(fs::read("sound/assets/voice_timeout.wav")?));

    // Said firstly because the queue starts playing immediately.
    queue
        .sound(
            Some(Cursor::new(connected.clone())),
            "connected".to_string(),
        )
        .push()?;
    // Said thirdly because it's pre-empted by the next sound.
    queue
        .sound(
            Some(Cursor::new(connected.clone())),
            "connected".to_string(),
        )
        .volume(0.3)
        .push()?;
    // Give it some time to start playing the first sound.
    sleep(Duration::from_millis(50)).await;
    // Said secondly because it has a higher priority than all pending sounds in
    // the queue.
    queue
        .sound(Some(Cursor::new(timeout.clone())), "timeout".to_string())
        .priority(1)
        .push()?;
    // Not said because it doesn't meet the `max_delay`.
    assert!(
        !queue
            .sound(Some(Cursor::new(timeout.clone())), "timeout".to_string())
            .priority(1)
            .max_delay(Duration::from_secs(1))
            .push()?
            .await
    );
    sleep(Duration::from_millis(250)).await;
    // Said lastly and blocks until said.
    assert!(
        queue
            .sound(
                Some(Cursor::new(connected.clone())),
                "connected".to_string()
            )
            .cancel_all()
            .push()?
            .await
    );

    // In result the queue should be played in the following order: connected,
    // timeout, connected, connected.

    Ok(())
}
