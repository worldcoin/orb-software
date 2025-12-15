//! Implementation of secure storage backend that uses tokio::process::Command

use std::{process::Stdio, sync::Arc};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::secure_storage::{RequestChannelPayload, Response, SecureStorage, CA_EUID};

use super::Request;

const BINARY_NAME: &str = "orb-secure-storage-ca";

pub(super) fn spawn(
    request_queue_capacity: usize,
    cancel: CancellationToken,
) -> SecureStorage {
    let (request_tx, request_rx) =
        mpsc::channel::<RequestChannelPayload>(request_queue_capacity);
    let cancel_clone = cancel.clone();
    tokio::task::spawn(async move {
        cancel_clone
            .run_until_cancelled_owned(task_entry(request_rx))
            .await;
        info!("all `SecureStorage` handles were dropped, killing task");
    });

    SecureStorage {
        request_tx,
        _drop_guard: Arc::new(cancel.drop_guard()),
    }
}

// Returns when the sending channel is dropped
async fn task_entry(mut request_rx: mpsc::Receiver<RequestChannelPayload>) {
    while let Some((request, response_tx)) = request_rx.recv().await {
        let mut cmd = tokio::process::Command::new(BINARY_NAME);
        cmd.uid(CA_EUID)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped());

        let mut child = match &request {
            Request::Put { key, .. } => cmd.args(&["put", key]),
            Request::Get { key } => cmd.args(&["get", key]),
        }
        .spawn()
        .expect("failed to spawn command");

        let mut child_stdin = child.stdin.take().unwrap();
        let mut child_stdout = child.stdout.take().unwrap();

        match &request {
            Request::Put { val, .. } => {
                child_stdin
                    .write_all(val)
                    .await
                    .expect(&format!("failed to write to {BINARY_NAME} stdin"));
                child_stdin
                    .flush()
                    .await
                    .expect(&format!("failed to flush stdin for {BINARY_NAME}"));
            }
            _ => (),
        }

        let mut output = Vec::new();
        child_stdout
            .read_to_end(&mut output)
            .await
            .expect(&format!("failed to read stdout from {BINARY_NAME}"));

        let response = match request {
            Request::Put { .. } => Response::Put(Ok(output)),
            Request::Get { .. } => Response::Get(Ok(output)),
        };

        let _ = response_tx.send(response); // We dont care if the receiver was dropped
    }
}
