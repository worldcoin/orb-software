//! Implementation of secure storage backend that uses tokio::process::Command

use std::{process::Stdio, sync::Arc};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::secure_storage::{RequestChannelPayload, SecureStorage, CA_EUID};

use super::Request;

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
        let mut cmd = tokio::process::Command::new("orb-secure-storage-ca");
        cmd.uid(CA_EUID)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped());

        let child = match request {
            Request::Put { key, val } => cmd.args(&["put", &key, todo!("value")]),
            Request::Get { key } => cmd.args(&["get", &key]),
        }
        .spawn()
        .expect("failed to spawn command");
    }
}
