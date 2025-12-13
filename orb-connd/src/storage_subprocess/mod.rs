mod messages;

use color_eyre::eyre::Result;
use futures::{Sink as _, SinkExt as _, StreamExt as _, TryStreamExt as _};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::storage_subprocess::messages::{Request, Response};

pub async fn entry(
    io: impl AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
) -> Result<()> {
    assert!(
        !rustix::process::geteuid().is_root(),
        "should not be running as root"
    );

    let length_delimited = tokio_util::codec::Framed::new(
        io,
        tokio_util::codec::length_delimited::LengthDelimitedCodec::default(),
    );
    let mut framed = tokio_serde::Framed::<_, Request, Response, _>::new(
        length_delimited,
        tokio_serde::formats::Cbor::<Request, Response>::default(),
    );

    let client = orb_secure_storage_ca::Client::new()?;

    while let Some(input) = framed.try_next().await? {
        match input {
            Request::Put { key, val } => {
                let response = handle_put(&key, &val).await;
                framed.send(response).await
            }
            Request::Get { key } => todo!(),
        }
    }

    Ok(())
}

async fn handle_put(key: &str, val: &[u8]) -> Response {
    todo!()
}
