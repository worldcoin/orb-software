pub mod client;
pub mod dbus;
pub mod logging;
pub mod remote_api;

use std::sync::Arc;

use eyre::{
    self,
    WrapErr,
};
use futures::FutureExt;
use tokio::{
    select,
    sync::Notify,
    time::sleep,
};
use tracing::{
    info,
    warn,
};
use url::Url;

#[cfg(feature = "prod")]
const BASE_AUTH_URL: &str = "https://auth.orb.worldcoin.org/api/v1/";
#[cfg(not(feature = "prod"))]
const BASE_AUTH_URL: &str = "https://auth.stage.orb.worldcoin.org/api/v1/";

#[cfg(feature = "prod")]
const PING_URL: &str = "https://management.orb.worldcoin.org/api/v1/orbs/";
#[cfg(not(feature = "prod"))]
const PING_URL: &str = "https://management.stage.orb.worldcoin.org/api/v1/orbs/";

#[tokio::main]
async fn main() -> eyre::Result<()> {
    logging::init();

    info!("Build Timestamp: {}", env!("VERGEN_BUILD_TIMESTAMP"));
    info!("Version: {}", env!("VERGEN_BUILD_SEMVER"));
    info!("git sha: {}", env!("VERGEN_GIT_SHA"));
    #[cfg(feature = "prod")]
    info!("build for PROD backend");
    #[cfg(not(feature = "prod"))]
    info!("build for STAGING backend");

    let base_url = Url::parse(BASE_AUTH_URL).wrap_err("can't parse BASE_AUTH_URL")?;
    let orb_id = std::env::var("ORB_ID").wrap_err("env variable `ORB_ID` should be set")?;
    let ping_url = Url::parse(PING_URL)
        .wrap_err("can't parse BASE_AUTH_URL")?
        .join(&orb_id)?;

    let force_refresh_token = Arc::new(Notify::new());

    let iface_ref = setup_dbus(force_refresh_token.clone())
        .await
        .wrap_err("Initialization failed")?;

    run(
        &orb_id,
        iface_ref,
        force_refresh_token.clone(),
        base_url,
        ping_url,
    )
    .await
    .wrap_err("mainloop failed")
}

/// Return either a *proovenly working* static token, or a short lived token.
#[tracing::instrument]
async fn get_working_token(
    orb_id: &str,
    base_url: &Url,
    ping_url: &Url,
) -> crate::remote_api::Token {
    select! {
        Ok(token) = get_working_static_token(orb_id, ping_url) => token,
        token = remote_api::get_token(orb_id, base_url) => token,
    }
}

/// Return proovenly working static token, or error if the token was rejected by the backend.
#[tracing::instrument]
async fn get_working_static_token(
    orb_id: &str,
    ping_url: &Url,
) -> std::io::Result<crate::remote_api::Token> {
    let token = remote_api::Token::from_usr_persistent().await?;
    let mut failure_counter = 0;
    // Loop until we get confirmation from the backend that the token is valid
    // or not. In case of network errors, keep trying.
    info!("got static token {token:#?}, validating it");
    loop {
        match crate::client::validate_token(orb_id, &token, ping_url).await {
            Ok(true) => {
                info!("Static token is valid");
                return Ok(token);
            }
            // TODO make this error more specific
            Ok(false) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "token was rejected by the backend",
                ));
            }
            Err(e) => {
                failure_counter += 1;
                warn!(error=?e, "Token validation has failed {} times.", failure_counter);
                continue;
            }
        }
    }
}

#[tracing::instrument]
async fn setup_dbus(
    force_refresh_token: Arc<Notify>,
) -> eyre::Result<zbus::InterfaceRef<dbus::AuthTokenManager>> {
    let dbus = dbus::create_dbus_connection(force_refresh_token)
        .await
        .wrap_err("failed to create DBus connection")?;

    let object_server = dbus.object_server();
    let iface_ref = object_server
        .interface::<_, dbus::AuthTokenManager>("/org/worldcoin/AuthTokenManager1")
        .await
        .wrap_err("failed to get reference to AuthTokenManager1 from object server")?;

    Ok(iface_ref)
}

async fn run(
    orb_id: &str,
    iface_ref: zbus::InterfaceRef<dbus::AuthTokenManager>,
    force_refresh_token: Arc<Notify>,
    base_url: Url,
    ping_url: Url,
) -> eyre::Result<()> {
    loop {
        let token = get_working_token(orb_id, &base_url, &ping_url).await;
        let token_refresh_delay = token.get_best_refresh_time();
        // get_mut() blocks access to the iface_ref object. So we never bind its result to be safe.
        // https://docs.rs/zbus/3.7.0/zbus/struct.InterfaceRef.html#method.get_mut
        iface_ref.get_mut().await.update_token(&token.token);
        iface_ref
            .get_mut()
            .await
            .token_changed(iface_ref.signal_context())
            .await
            .wrap_err("failed to send token_changed signal")?;

        //  Wait for whatever happens first: token expires or a refresh is requested
        select! {
            () = sleep(token_refresh_delay).fuse() => {info!("token is about to expire, refreshing it");},
            _ = force_refresh_token.notified().fuse() => {info!("refresh was requested, refreshing the token");},
        };
    }
}
