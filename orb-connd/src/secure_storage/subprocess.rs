//! Implementation of secure storage backend using a fork/exec subprocess.

use crate::secure_storage::messages::{GetErr, PutErr, Request, Response};
use crate::secure_storage::{ConndStorageScopes, RequestChannelPayload, SecureStorage};
use color_eyre::eyre::{eyre, Result};
use futures::{Sink, SinkExt as _, Stream, TryStreamExt as _};
use orb_secure_storage_ca::BackendT;
use std::io::Result as IoResult;
use std::path::PathBuf;
use std::{process::Stdio, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

type SsClient<B> = Arc<std::sync::Mutex<orb_secure_storage_ca::Client<B>>>;

pub(super) fn spawn(
    exe_path: PathBuf,
    in_memory: bool,
    request_queue_size: usize,
    cancel: CancellationToken,
    scope: ConndStorageScopes,
) -> SecureStorage {
    let mut framed_pipes = make_framed_subprocess(exe_path, in_memory, scope);
    let (request_tx, mut request_rx) =
        mpsc::channel::<RequestChannelPayload>(request_queue_size);
    let cancel_clone = cancel.clone();

    tokio::task::spawn(async move {
        let io_fut = async move {
            while let Some((request, response_tx)) = request_rx.recv().await {
                framed_pipes
                    .send(request)
                    .await
                    .expect("error while communicating with subprocess via pipe");
                let response = framed_pipes
                    .try_next()
                    .await
                    .expect("error while communicating with subprocess via pipe")
                    .expect("subprocess pipe unexpectedly closed");

                let _ = response_tx.send(response); // we don't care if the receiver was dropped
            }
        };
        tokio::select! {
            () = io_fut => {},
            () = cancel_clone.cancelled() => {},
        }

        info!("all `SecureStorage` handles were dropped, killing task");
    });

    SecureStorage {
        request_tx,
        _drop_guard: Arc::new(cancel.drop_guard()),
    }
}

fn make_framed_subprocess(
    exe_path: PathBuf,
    in_memory: bool,
    scope: ConndStorageScopes,
) -> impl Stream<Item = IoResult<Response>> + Sink<Request, Error = std::io::Error> {
    let current_euid = rustix::process::geteuid();
    let current_egid = rustix::process::getegid();
    let (child_euid, child_egid) = if current_euid.is_root() {
        let child_username = scope.as_username();
        let child_groupname = scope.as_groupname();
        let child_euid = uzers::get_user_by_name(child_username)
            .ok_or_else(|| eyre!("username {child_username} doesn't exist"))
            .unwrap()
            .uid();
        let child_egid = uzers::get_group_by_name(child_groupname)
            .ok_or_else(|| eyre!("username {child_groupname} doesn't exist"))
            .unwrap()
            .gid();

        (child_euid, child_egid)
    } else {
        warn!("current EUID in parent connd process is not root! For this reason we will spawn the subprocess as the same EUID, since we don't have perms to change it. This probably only should be done in integration tests." );
        (current_euid.as_raw(), current_egid.as_raw())
    };

    let mut cmd = tokio::process::Command::new(exe_path);
    cmd.arg("secure-storage-worker")
        .args(["--scope", &scope.to_string()])
        .uid(child_euid)
        .gid(child_egid)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    if in_memory {
        cmd.arg("--in-memory");
    }
    let mut child = cmd
        .spawn()
        .expect("failed to spawn secure storage subprocess");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let io = tokio::io::join(stdout, stdin);
    // codec converts Stream/Sink -> Read/Write
    let length_delimited = tokio_util::codec::Framed::new(
        io,
        tokio_util::codec::length_delimited::LengthDelimitedCodec::default(),
    );

    tokio_serde::Framed::<_, Response, Request, _>::new(
        length_delimited,
        tokio_serde::formats::Cbor::<Response, Request>::default(),
    )
}

/// The entry point of the subprocess
pub async fn entry<B>(
    io: impl AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
    secure_storage_context: &mut B::Context,
    scope: ConndStorageScopes,
) -> Result<()>
where
    B: BackendT + 'static,
    B::Session: Send + 'static,
{
    let euid = rustix::process::geteuid();
    info!(
        "secure storage entry point, running as user {}",
        euid.as_raw()
    );
    assert!(!euid.is_root(), "should not be running as root");

    // codec converts Read/Write -> Stream/Sink
    let length_delimited = tokio_util::codec::Framed::new(
        io,
        tokio_util::codec::length_delimited::LengthDelimitedCodec::default(),
    );
    let mut framed = tokio_serde::Framed::<_, Request, Response, _>::new(
        length_delimited,
        tokio_serde::formats::Cbor::<Request, Response>::default(),
    );

    // A bit lame to use a mutex just for `spawn_blocking()` but /shrug
    let client: SsClient<B> = Arc::new(std::sync::Mutex::new(
        orb_secure_storage_ca::Client::new(secure_storage_context, scope.as_domain())?,
    ));

    while let Some(input) = framed.try_next().await? {
        let response = match input {
            Request::Put { key, val } => handle_put(client.clone(), key, val).await,
            Request::Get { key } => handle_get(client.clone(), key).await,
        };
        debug!("response: {response:?}");
        framed.send(response).await?;
    }

    Ok(())
}

async fn handle_put<B>(client: SsClient<B>, key: String, val: Vec<u8>) -> Response
where
    B: BackendT + 'static,
    B::Session: Send + 'static,
{
    debug!("PutRequest: key={}, value_len={}", key, val.len());
    let result =
        tokio::task::spawn_blocking(move || client.lock().unwrap().put(&key, &val))
            .await
            .expect("task panicked")
            .map_err(|err| PutErr::Generic(format!("{err:?}")));

    Response::Put(result)
}

async fn handle_get<B>(client: SsClient<B>, key: String) -> Response
where
    B: BackendT + 'static,
    B::Session: Send + 'static,
{
    debug!("GetRequest: key={key}");
    let result = tokio::task::spawn_blocking(move || client.lock().unwrap().get(&key))
        .await
        .expect("task panicked")
        .map_err(|err| GetErr::Generic(format!("{err:?}")));

    Response::Get(result)
}
