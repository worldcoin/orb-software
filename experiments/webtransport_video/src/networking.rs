///! All networking related code
use std::{
    net::{Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
};

use axum::{routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use color_eyre::{eyre::WrapErr as _, Result};

use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument};
use wtransport::{endpoint::IncomingSession, tls::Sha256DigestFmt};

use crate::{
    video::{EncodedPng, Video},
    Args,
};

#[instrument(skip_all)]
pub async fn run_http_server(
    args: Args,
    cancel: CancellationToken,
    identity: wtransport::Identity,
) -> Result<()> {
    let _cancel_guard = cancel.drop_guard();
    let app = Router::new().route("/", get(|| async { "Hello, world!" }));

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
    let conn = session_request
        .accept()
        .await
        .wrap_err("failed to accept incoming connection")?;

    while let Ok(()) = png_rx.changed().await {
        // We use an arc to avoid expensive frame copies.
        let png = png_rx.borrow_and_update().clone();
        conn.send_datagram(png.clone_cheap())
            .wrap_err("failed to send dgram")?;
    }
    // Channel cloded, shut down task.

    Ok(())
}
