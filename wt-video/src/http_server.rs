use std::{
    net::{Ipv6Addr, SocketAddr},
    pin::pin,
    time::Duration,
};

use axum::{extract::State, response::Html, routing::get, Router};
use axum_extra::response::JavaScript;
use axum_server::tls_rustls::RustlsConfig;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::FutureExt as _;
use tokio::{select, time::timeout};
use tokio_util::sync::CancellationToken;

/// One-stop shop for spawning the HTTP server.
#[derive(Debug, bon::Builder)]
#[must_use]
#[builder(state_mod(vis = ""))]
pub struct Config {
    pub cancel: CancellationToken,
    pub port: u16,
    #[builder(getter(vis = ""))]
    pub wt_identity: wtransport::Identity,
    pub tls_config: RustlsConfig,
}
use config_builder as builder;
use tracing::{debug, instrument};

impl<S: builder::State> ConfigBuilder<S> {
    pub async fn tls_config_from_wt_identity(
        self,
    ) -> ConfigBuilder<builder::SetTlsConfig<S>>
    where
        S::WtIdentity: builder::IsSet,
        S::TlsConfig: builder::IsUnset,
    {
        let wt_identity = self.get_wt_identity();
        let cert = wt_identity.certificate_chain().as_slice()[0]
            .to_pem()
            .as_bytes()
            .to_vec();
        let key = wt_identity
            .private_key()
            .to_secret_pem()
            .as_bytes()
            .to_vec();
        let tls_config = RustlsConfig::from_pem(cert, key).await.unwrap();

        self.tls_config(tls_config)
    }
}

impl Config {
    pub async fn spawn(self) -> Result<HttpServerHandle> {
        HttpServerHandle::spawn(self).await
    }
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            cancel: self.cancel.clone(),
            port: self.port.clone(),
            wt_identity: self.wt_identity.clone_identity(),
            tls_config: self.tls_config.clone(),
        }
    }
}

#[derive(Debug)]
pub struct HttpServerHandle {
    pub task_handle: tokio::task::JoinHandle<Result<()>>,
    pub local_addr: SocketAddr,
}

impl HttpServerHandle {
    pub async fn spawn(cfg: Config) -> Result<Self> {
        let axum_handle = axum_server::Handle::new();
        let cancel_clone = cfg.cancel.clone();
        let server_task_handle = spawn_wrapped_server_task(axum_handle.clone(), cfg);
        let Some(local_addr) =
            timeout(Duration::from_millis(10000), axum_handle.listening())
                .await
                .expect("should not hang that long ever")
        else {
            cancel_clone.cancel();
            let err =
                async { server_task_handle.await.wrap_err("wrapped task panicked")? }
                    .await
                    .expect_err("should be an error");
            return Err(err).wrap_err("http server failed to bind");
        };
        Ok(Self {
            local_addr,
            task_handle: server_task_handle,
        })
    }
}

fn spawn_wrapped_server_task(
    axum_handle: axum_server::Handle,
    cfg: Config,
) -> tokio::task::JoinHandle<Result<()>> {
    let cancel = cfg.cancel.clone();
    let server_task = tokio::task::spawn(task_entry(axum_handle.clone(), cfg));
    let outer_task = tokio::task::spawn(async move {
        let mut server_fut = pin!(async {
            server_task
                .await
                .wrap_err("http task panicked")?
                .wrap_err("http task returned error")
        }
        .fuse());
        select! {
            biased; () = cancel.cancelled() => {
                debug!("shutting down http task");
                axum_handle.graceful_shutdown(Some(Duration::from_millis(1000)));
                server_fut.await
            },
            server_result = &mut server_fut => server_result,
        }
    });

    outer_task
}

#[instrument(skip_all)]
async fn task_entry(handle: axum_server::Handle, cfg: Config) -> Result<()> {
    let _cancel_guard = cfg.cancel.drop_guard();

    let addr = SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), cfg.port);

    let app = make_axum_router(&cfg.wt_identity);

    axum_server::bind_rustls(addr, cfg.tls_config)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .wrap_err("error in http server")
}

/// Allows nesting the service inside other axum routers.
pub fn make_axum_router(wt_identity: &wtransport::Identity) -> axum::Router {
    Router::new()
        .route("/api/cert_hash", get(cert_hash))
        .route("/", get(index_html))
        .route("/index.html", get(index_html))
        .route("/index.js", get(index_js))
        .with_state(RouterState {
            cert_hash: wt_identity.certificate_chain().as_slice()[0].hash(),
        })
}

#[derive(Debug, Clone)]
struct RouterState {
    cert_hash: wtransport::tls::Sha256Digest,
}

/// Endpoint to retrieve the servers cert hash. This will allow us to bootstrap
/// entirely from just self-signed https.
#[instrument]
async fn cert_hash(State(state): State<RouterState>) -> [u8; 32] {
    *state.cert_hash.as_ref()
}

async fn index_js() -> JavaScript<&'static str> {
    JavaScript(include_str!("../index.js"))
}

async fn index_html() -> Html<&'static str> {
    Html(include_str!("../index.html"))
}
