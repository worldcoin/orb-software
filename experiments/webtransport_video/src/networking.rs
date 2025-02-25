//! All networking related code
use std::{
    net::{Ipv6Addr, SocketAddr},
    time::Duration,
};

use axum::{extract::State, routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::Stream;
use futures::StreamExt as _;
use tokio::{select, time::timeout};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument, trace};
use wtransport::endpoint::IncomingSession;

use crate::{control::InputFrame, Args, EncodedPng};

// We box it to make the types slightly simpler
type ControlStream = Box<dyn Stream<Item = Result<InputFrame>> + Send + Sync + Unpin>;

#[derive(Debug, Clone)]
struct RouterState {
    cert_hash: wtransport::tls::Sha256Digest,
}

/// Endpoint to retrieve the servers cert hash. This will allow us to bootstrap
/// entirely from just self-signed https.
async fn cert_hash(State(state): State<RouterState>) -> [u8; 32] {
    *state.cert_hash.as_ref()
}

#[instrument(skip_all)]
pub async fn run_http_server(
    args: Args,
    cancel: CancellationToken,
    identity: wtransport::Identity,
) -> Result<()> {
    let _cancel_guard = cancel.drop_guard();

    let app = Router::new()
        .route("/cert_hash", get(cert_hash))
        .with_state(RouterState {
            cert_hash: identity.certificate_chain().as_slice()[0].hash(),
        })
        .fallback_service(tower_http::services::ServeDir::new("."));

    let cert = identity.certificate_chain().as_slice()[0]
        .to_pem()
        .as_bytes()
        .to_vec();
    let key = identity.private_key().to_secret_pem().as_bytes().to_vec();
    let config = RustlsConfig::from_pem(cert, key).await.unwrap();

    let addr = SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), args.http_port);
    info!("listening on {}", addr);
    axum_server::tls_rustls::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await
        .wrap_err("error in http server")
}

#[instrument(skip_all)]
pub async fn run_wt_server(
    args: Args,
    cancel: CancellationToken,
    identity: wtransport::Identity,
    png_rx: tokio::sync::watch::Receiver<EncodedPng>,
) -> Result<()> {
    let _cancel_guard = cancel.drop_guard();

    let server_config = wtransport::ServerConfig::builder()
        .with_bind_default(args.wt_port)
        .with_identity(identity)
        .build();
    let server = wtransport::Endpoint::server(server_config)
        .wrap_err("failed to bind webtransport server")?;
    info!(
        "started webtransport server on {}",
        server.local_addr().unwrap()
    );

    let mut connection_tasks = tokio::task::JoinSet::new();
    loop {
        let incoming_session = server.accept().await;
        let png_rx_clone = png_rx.clone();
        connection_tasks.spawn(async move {
            if let Err(err) = conn_task(incoming_session, png_rx_clone).await {
                error!(?err, "conn task failed");
            }
        });
    }
}

async fn accept_control_stream(
    conn: &mut wtransport::Connection,
) -> Result<ControlStream> {
    let incoming_stream = timeout(Duration::from_millis(2000), conn.accept_uni())
        .await
        .wrap_err("timed out waiting for incoming input stream")?
        .wrap_err("error receiving incoming input stream")?;
    let length_framed = tokio_util::codec::FramedRead::new(
        incoming_stream,
        tokio_util::codec::length_delimited::LengthDelimitedCodec::new(),
    );
    let json_codec = tokio_serde::formats::Json::<InputFrame, ()>::default();
    let serde_framed: tokio_serde::Framed<_, InputFrame, (), _> =
        tokio_serde::Framed::new(length_framed, json_codec);
    Ok(Box::new(serde_framed.map(|item| {
        item.wrap_err("failed to get next item in input stream")
    })))
}

#[instrument(skip_all, fields(remote_address = format!("{}", incoming_session.remote_address())))]
async fn conn_task(
    incoming_session: IncomingSession,
    mut png_rx: tokio::sync::watch::Receiver<EncodedPng>,
) -> Result<()> {
    let session_request = incoming_session
        .await
        .wrap_err("failed to accept incoming session")?;
    info!(
        headers = ?session_request.headers(),
        address = ?session_request.remote_address(),
        user_agent = ?session_request.user_agent(),
        "incoming session request"
    );
    let mut conn = session_request
        .accept()
        .await
        .wrap_err("failed to accept incoming connection")?;

    let mut control_stream = accept_control_stream(&mut conn)
        .await
        .wrap_err("failed to accept incoming control stream")?;

    loop {
        select! {
            png_result = png_rx.changed() => {
                if png_result.is_err() {
                    info!("video stream closed, shutting down");
                    break;
                }
                let png = png_rx.borrow().clone_cheap();
                on_video_frame(&mut conn, png).await.wrap_err("failed to send latest video frame")?
            },
            control_result = control_stream.next() => {
                let Some(result) = control_result else {
                    info!("control stream closed, shutting down");
                    break;
                };
                let command: InputFrame = result.wrap_err("failed to receive control input")?;
                on_command(command);
            }
        }
    }

    Ok(())
}

fn on_command(command: InputFrame) {
    // TODO: actually do something meaningful instead of logging it
    info!(?command, "got command");
}

async fn on_video_frame(
    conn: &mut wtransport::Connection,
    png: EncodedPng,
) -> Result<()> {
    let mut video_stream = conn
        .open_uni()
        .await
        .wrap_err("failed to allocate stream from flow control")?
        .await
        .wrap_err("failed open stream")?;
    trace!("writing {} bytes", png.as_slice().len());
    video_stream
        .write_all(png.as_slice())
        .await
        .wrap_err("failed to write to stream")?;
    video_stream
        .finish()
        .await
        .wrap_err("failed to finish stream")?;

    Ok(())
}
