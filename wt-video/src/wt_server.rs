//! Webtransport server

use std::{net::SocketAddr, time::Duration};

use color_eyre::{eyre::WrapErr as _, Result};
use futures::{Stream, StreamExt as _};
use tokio::time::error::Elapsed;
use tokio::{
    select,
    sync::{
        mpsc::{self, error::SendError},
        watch,
    },
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tracing::{
    debug, error, info, info_span, instrument, trace, warn, Instrument as _,
};
use wtransport::{endpoint::IncomingSession, VarInt};

use crate::{control::ControlEvent, EncodedPng};

// We box it to make the types slightly simpler
type ControlStream = Box<dyn Stream<Item = Result<ControlEvent>> + Send + Sync + Unpin>;

#[derive(Debug, bon::Builder)]
pub struct Config {
    pub port: u16,
    pub identity: wtransport::Identity,
    pub png_rx: watch::Receiver<EncodedPng>,
    pub control_tx: mpsc::Sender<ControlEvent>,
    pub cancel: CancellationToken,
}
use config_builder as builder;

impl<S: builder::State> ConfigBuilder<S> {
    pub fn identity_self_signed(
        self,
        subject_alt_names: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> ConfigBuilder<builder::SetIdentity<S>>
    where
        S::Identity: builder::IsUnset,
    {
        let identity = wtransport::Identity::self_signed_builder()
            .subject_alt_names(subject_alt_names)
            .from_now_utc()
            .validity_days(7)
            .build()
            .unwrap();

        self.identity(identity)
    }
}

impl Config {
    pub fn bind(self) -> Result<WtServer> {
        WtServer::new(self)
    }
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            control_tx: self.control_tx.clone(),
            port: self.port,
            identity: self.identity.clone_identity(),
            png_rx: self.png_rx.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

#[must_use]
pub struct WtServer {
    cancel: CancellationToken,
    endpoint: wtransport::Endpoint<wtransport::endpoint::endpoint_side::Server>,
    local_addr: SocketAddr,
    png_rx: watch::Receiver<EncodedPng>,
    control_tx: mpsc::Sender<ControlEvent>,
}

impl WtServer {
    pub fn new(cfg: Config) -> Result<Self> {
        let server_config = wtransport::ServerConfig::builder()
            .with_bind_default(cfg.port)
            .with_identity(cfg.identity.clone_identity())
            .build();
        let server = wtransport::Endpoint::server(server_config)
            .wrap_err("failed to bind webtransport server")?;
        let local_addr = server.local_addr().unwrap();

        Ok(Self {
            cancel: cfg.cancel,
            png_rx: cfg.png_rx,
            endpoint: server,
            local_addr,
            control_tx: cfg.control_tx,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    #[instrument(skip_all)]
    pub async fn run(self) -> Result<()> {
        let _cancel_guard = self.cancel.clone().drop_guard();

        let mut connection_tasks = tokio::task::JoinSet::new();
        let accept_loop_fut = async {
            loop {
                let incoming_session = self.endpoint.accept().await;
                let png_rx_clone = self.png_rx.clone();
                // TODO: Should we diambiguate each client as a separate control stream?
                let control_tx_clone = self.control_tx.clone();
                let remote_address = incoming_session.remote_address();
                connection_tasks.spawn(
                    async move {
                        if let Err(err) =
                            conn_task(incoming_session, png_rx_clone, control_tx_clone)
                                .await
                        {
                            error!(?err, "conn task failed");
                        }
                    }
                    .instrument(info_span!("conn task", %remote_address)),
                );
            }
        };
        select! {
            _ = accept_loop_fut => unreachable!("this loop never breaks"),
            () = self.cancel.cancelled() => connection_tasks.shutdown().await,
        }
        debug!("webtransport future cancelled, shutting down...");

        self.endpoint
            .close(VarInt::from_u32(0), b"server shutting down");
        if let Err(Elapsed { .. }) =
            timeout(Duration::from_millis(1000), self.endpoint.wait_idle()).await
        {
            warn!("timed out waiting for webtransport server to cleanly shutdown");
        }
        Ok(())
    }
}

async fn accept_control_stream(
    conn: &mut wtransport::Connection,
) -> Result<ControlStream> {
    let incoming_stream = timeout(Duration::from_millis(2000), conn.accept_uni())
        .await
        .wrap_err("timed out waiting for incoming control stream")?
        .wrap_err("error receiving incoming control stream")?;
    let length_framed = tokio_util::codec::FramedRead::new(
        incoming_stream,
        tokio_util::codec::length_delimited::LengthDelimitedCodec::new(),
    );
    let json_codec = tokio_serde::formats::Json::<ControlEvent, ()>::default();
    let serde_framed: tokio_serde::Framed<_, ControlEvent, (), _> =
        tokio_serde::Framed::new(length_framed, json_codec);
    Ok(Box::new(serde_framed.map(|item| {
        item.wrap_err("failed to get next item in control stream")
    })))
}

async fn conn_task(
    incoming_session: IncomingSession,
    mut png_rx: watch::Receiver<EncodedPng>,
    control_tx: mpsc::Sender<ControlEvent>,
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
            ui_rx_result = control_stream.next() => {
                let Some(result) = ui_rx_result else {
                    info!("control stream closed, shutting down");
                    break;
                };
                let command: ControlEvent = result.wrap_err("failed to receive control input")?;
                if let Err(SendError(_)) = control_tx.send(command).await {
                    info!("all control event receivers closed, shutting down");
                    break;
                }

            }
        }
    }

    Ok(())
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
