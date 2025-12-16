//! Implementation of secure storage backend using a fork/exec subprocess.

use crate::secure_storage::messages::{GetErr, PutErr, Request, Response};
use crate::secure_storage::{RequestChannelPayload, SecureStorage, CA_EUID};
use color_eyre::eyre::Result;
use futures::{Sink, SinkExt as _, Stream, TryStreamExt as _};
use orb_secure_storage_ca::BackendT;
use std::io::Result as IoResult;
use std::path::Path;
use std::{process::Stdio, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

type SsClient<B> = Arc<std::sync::Mutex<orb_secure_storage_ca::Client<B>>>;

pub(super) fn spawn(
    exe_path: impl AsRef<Path> + 'static,
    in_memory: bool,
    request_queue_size: usize,
    cancel: CancellationToken,
) -> SecureStorage {
    let mut framed_pipes = make_framed_subprocess(exe_path, in_memory);
    // TODO: perhaps this should always be 1 or 0?
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
    exe_path: impl AsRef<Path>,
    in_memory: bool,
) -> impl Stream<Item = IoResult<Response>> + Sink<Request, Error = std::io::Error> {
    let current_euid = rustix::process::geteuid();
    let child_euid = if current_euid.is_root() {
        CA_EUID
    } else {
        warn!("current EUID in parent connd process is not root! For this reason we will spawn the subprocess as the same EUID, since we don't have perms to change it. This probably only should be done in integration tests." );
        current_euid.as_raw()
    };

    let current_exe = std::env::current_exe().expect("infallible");
    let mut child = tokio::process::Command::new(exe_path.as_ref())
        .arg("ssd")
        .arg(format!("--in-memory={in_memory}"))
        .uid(child_euid)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
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
        orb_secure_storage_ca::Client::new(secure_storage_context)?,
    ));

    while let Some(input) = framed.try_next().await? {
        info!("request: {input:?}");
        let response = match input {
            Request::Put { key, val } => handle_put(client.clone(), key, val).await,
            Request::Get { key } => handle_get(client.clone(), key).await,
        };
        info!("response: {response:?}");
        framed.send(response).await?;
    }

    Ok(())
}

async fn handle_put<B>(client: SsClient<B>, key: String, val: Vec<u8>) -> Response
where
    B: BackendT + 'static,
    B::Session: Send + 'static,
{
    let result =
        tokio::task::spawn_blocking(move || client.lock().unwrap().put(&key, &val))
            .await
            .expect("task panicked")
            .map_err(|err| PutErr::Generic(err.to_string()));

    Response::Put(result)
}

async fn handle_get<B>(client: SsClient<B>, key: String) -> Response
where
    B: BackendT + 'static,
    B::Session: Send + 'static,
{
    let result = tokio::task::spawn_blocking(move || client.lock().unwrap().get(&key))
        .await
        .expect("task panicked")
        .map_err(|err| GetErr::Generic(err.to_string()));

    Response::Get(result)
}
