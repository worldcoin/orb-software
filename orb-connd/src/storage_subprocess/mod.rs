pub mod messages;

use std::io::Result as IoResult;
use std::{process::Stdio, sync::Arc, time::Duration};

use color_eyre::eyre::{Report, Result};
use futures::{Sink, SinkExt as _, Stream, TryStreamExt as _};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::error::Elapsed,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::{
    storage_subprocess::messages::{GetErr, PutErr, Request, Response},
    EntryPoint,
};

type SsClient = Arc<std::sync::Mutex<orb_secure_storage_ca::Client>>;

const EUID: u32 = 1000;

pub fn spawn_from_parent(
) -> impl Stream<Item = IoResult<Response>> + Sink<Request, Error = std::io::Error> {
    let current_exe = std::env::current_exe().expect("infallible");
    let mut child = tokio::process::Command::new(current_exe)
        .env(
            crate::ENV_FORK_MARKER,
            (EntryPoint::SecureStorage as u8).to_string(),
        )
        .uid(EUID)
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
    let framed = tokio_serde::Framed::<_, Response, Request, _>::new(
        length_delimited,
        tokio_serde::formats::Cbor::<Response, Request>::default(),
    );

    framed
}

/// The entry point of the subprocess
pub async fn entry(
    io: impl AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
) -> Result<()> {
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

    // A bit lame to use a mutex but /shrug
    let client = Arc::new(std::sync::Mutex::new(orb_secure_storage_ca::Client::new()?));

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

async fn handle_put(client: SsClient, key: String, val: Vec<u8>) -> Response {
    let result =
        tokio::task::spawn_blocking(move || client.lock().unwrap().put(&key, &val))
            .await
            .expect("task panicked")
            .map(|_| ())
            .map_err(|err| PutErr::Generic(err.to_string()));

    Response::Put(result)
}

async fn handle_get(client: SsClient, key: String) -> Response {
    let result = tokio::task::spawn_blocking(move || client.lock().unwrap().get(&key))
        .await
        .expect("task panicked")
        .map_err(|err| GetErr::Generic(err.to_string()));

    Response::Get(result)
}
