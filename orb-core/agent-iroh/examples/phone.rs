//! Example code, as if it were the phone running iroh.

use color_eyre::Result;
use common::{phone_pubkey, AppProtocol};
use iroh::{protocol::Router, SecretKey};
use orb_agent_iroh::{BoxedHandler, EndpointConfig, RouterConfig};
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

mod common;

use crate::common::PHONE_SECRETKEY;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    info!("starting, accessible at {}", phone_pubkey());
    let secretkey = SecretKey::from_bytes(&PHONE_SECRETKEY);
    let _router = Config::builder()
        .endpoint_cfg(EndpointConfig::builder().secret_key(secretkey).build())
        .router_cfg(
            RouterConfig::builder()
                .handler(AppProtocol::ALPN, AppProtocol)
                .build(),
        )
        .build()
        .spawn_router()
        .await?;

    let _ = tokio::signal::ctrl_c().await;
    info!("exiting");

    Ok(())
}

#[derive(Debug, bon::Builder)]
struct Config {
    endpoint_cfg: EndpointConfig,
    router_cfg: RouterConfig,
}

impl Config {
    pub async fn spawn_router(self) -> Result<Router> {
        let endpoint = self.endpoint_cfg.bind().await?;
        let mut router = iroh::protocol::Router::builder(endpoint.clone());
        // Store these so we can clear the conn_tx later in response to [`Input`]s.
        for (alpn, handler) in self.router_cfg.handlers {
            let handler = BoxedHandler::from(handler);
            router = router.accept(alpn.0, handler);
        }
        let router = router.spawn();

        Ok(router)
    }
}
