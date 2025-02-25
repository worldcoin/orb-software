//! All networking related code
use std::net::{Ipv6Addr, SocketAddr};

use axum::{extract::State, routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use color_eyre::{eyre::WrapErr as _, Result};
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument};

use crate::Args;

#[derive(Debug, Clone)]
struct RouterState {
    cert_hash: wtransport::tls::Sha256Digest,
}

/// Endpoint to retrieve the servers cert hash. This will allow us to bootstrap
/// entirely from just self-signed https.
async fn cert_hash(State(state): State<RouterState>) -> [u8; 32] {
    *state.cert_hash.as_ref()
}

/// Allows nesting the service inside other axum routers.
pub fn make_axum_router(wt_identity: &wtransport::Identity) -> axum::Router {
    Router::new()
        .route("/cert_hash", get(cert_hash))
        .with_state(RouterState {
            cert_hash: wt_identity.certificate_chain().as_slice()[0].hash(),
        })
}

/// One-stop shop for spawning the HTTP server.
#[derive(Debug, bon::Builder)]
#[builder(state_mod(vis = "pub(super)"))]
struct HttpServerConfig {
    pub cancel: CancellationToken,
    pub port: u16,
    #[builder(getter(vis = ""))]
    pub wt_identity: wtransport::Identity,
    pub tls_config: RustlsConfig,
}

impl<S: http_server_config_builder::State> HttpServerConfigBuilder<S> {
    pub async fn tls_config_from_wt_identity(
        self,
    ) -> HttpServerConfigBuilder<http_server_config_builder::SetTlsConfig<S>>
    where
        S::WtIdentity: http_server_config_builder::IsSet,
        S::TlsConfig: http_server_config_builder::IsUnset,
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

struct HttpServerHandle {
    pub task_handle: tokio::task::JoinHandle<Result<()>>,
    pub local_addr: SocketAddr,
}

async fn task_entry(handle: axum_server::Handle, cfg: HttpServerConfig) -> Result<()> {
    let addr = SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), cfg.port);

    let cert = cfg.identity.certificate_chain().as_slice()[0]
        .to_pem()
        .as_bytes()
        .to_vec();
    let key = cfg
        .identity
        .private_key()
        .to_secret_pem()
        .as_bytes()
        .to_vec();
    let tls_config = RustlsConfig::from_pem(cert, key).await.unwrap();

    let app = make_axum_router(&cfg.identity)
        .fallback_service(tower_http::services::ServeDir::new("."));

    axum_server::bind_rustls(addr, tls_config)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .wrap_err("error in http server")
}
