use std::{
    io,
    path::Path,
    sync::{Arc, Mutex},
};

use tokio::{
    net::UnixDatagram,
    task::{self, JoinHandle},
};

const MAX_DATAGRAM_SIZE: usize = 8192;

pub struct Agent {
    received: Arc<Mutex<Vec<String>>>,
    handle: Option<JoinHandle<io::Result<()>>>,
}

impl Agent {
    pub async fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_owned();

        if tokio::fs::try_exists(&path).await? {
            tokio::fs::remove_file(&path).await?;
        }

        let socket = UnixDatagram::bind(&path)?;

        let received = Arc::new(Mutex::new(Vec::new()));

        let handle = task::spawn({
            let received = Arc::clone(&received);

            async move { receive(socket, received).await }
        });

        Ok(Self {
            received,
            handle: Some(handle),
        })
    }

    pub fn recvd(&self) -> Vec<String> {
        self.received.lock().expect("mutex poisoned").clone()
    }

    pub fn occurrences(&self, needle: &str) -> usize {
        self.received
            .lock()
            .expect("mutex poisoned")
            .iter()
            .map(|content| content.matches(needle).count())
            .sum()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

async fn receive(
    socket: UnixDatagram,
    received: Arc<Mutex<Vec<String>>>,
) -> io::Result<()> {
    let mut buf = [0; MAX_DATAGRAM_SIZE];

    loop {
        let len = socket.recv(&mut buf).await?;
        let content = String::from_utf8_lossy(&buf[..len]).into_owned();
        received.lock().expect("mutex poisoned").push(content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DogstatsdClient, MetricEmitter, NO_TAGS};
    use async_tempfile::TempDir;
    use std::{path::PathBuf, time::Duration};
    use tokio::time;

    #[tokio::test]
    async fn records_metrics_emitted_by_dogstatsd_client() {
        // Arrange
        let (_dir, _socket_path, agent, client) = setup().await;

        // Act
        client
            .count("first.metric", 1, NO_TAGS)
            .expect("count should enqueue");
        client
            .gauge("second.metric", 2.0, NO_TAGS)
            .expect("gauge should enqueue");

        // Assert
        wait_for_occurrences(&agent, "first.metric:1|c", 1).await;
        wait_for_occurrences(&agent, "second.metric:2|g", 1).await;
        assert_eq!(agent.occurrences("first.metric:1|c"), 1);
        assert_eq!(agent.occurrences("second.metric:2|g"), 1);
    }

    #[tokio::test]
    async fn counts_occurrences_across_metrics_emitted_by_dogstatsd_client() {
        // Arrange
        let (_dir, _socket_path, agent, client) = setup().await;

        // Act
        client
            .count("orb.metric", 1, ["env:test", "orb:test"])
            .expect("first count should enqueue");
        client
            .count("other.metric", 1, ["orb:test"])
            .expect("second count should enqueue");

        // Assert
        wait_for_occurrences(&agent, "orb", 3).await;
        assert_eq!(agent.occurrences("orb"), 3);
        assert_eq!(agent.occurrences("missing"), 0);
    }

    #[tokio::test]
    async fn replaces_existing_socket_file_and_receives_metrics() {
        // Arrange
        let dir = TempDir::new().await.expect("temp dir should be created");
        let socket_path = dir.join("dogstatsd.sock");
        let stale_socket =
            UnixDatagram::bind(&socket_path).expect("stale socket should bind");
        drop(stale_socket);

        // Act
        let agent = Agent::new(&socket_path)
            .await
            .expect("agent should replace stale socket");
        let client = client_for(&socket_path);
        client
            .incr("after.replace", NO_TAGS)
            .expect("increment should enqueue");

        // Assert
        wait_for_occurrences(&agent, "after.replace:1|c", 1).await;
        assert_eq!(agent.occurrences("after.replace:1|c"), 1);
    }

    async fn setup() -> (TempDir, PathBuf, Agent, DogstatsdClient) {
        let dir = TempDir::new().await.expect("temp dir should be created");
        let socket_path = dir.join("dogstatsd.sock");
        let agent = Agent::new(&socket_path).await.expect("agent should start");
        let client = client_for(&socket_path);

        (dir, socket_path, agent, client)
    }

    async fn wait_for_occurrences(agent: &Agent, needle: &str, expected: usize) {
        for _ in 0..100 {
            if agent.occurrences(needle) == expected {
                return;
            }

            time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(agent.occurrences(needle), expected);
    }

    fn client_for(socket_path: &Path) -> DogstatsdClient {
        DogstatsdClient::new_with(
            16,
            16,
            Duration::from_millis(1),
            socket_path.to_string_lossy().into_owned(),
            Duration::from_millis(1),
        )
    }
}
