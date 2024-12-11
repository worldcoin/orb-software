#![forbid(unsafe_code)]

pub mod client;
pub mod config;
pub mod dbus;
pub mod remote_api;

use std::sync::Arc;

use eyre::{self, bail, WrapErr};
use futures::{FutureExt, StreamExt};
use orb_build_info::{make_build_info, BuildInfo};
use secrecy::ExposeSecret;
use tokio::{select, sync::Notify, time::sleep};
use tracing::{info, warn};
use url::Url;

const BUILD_INFO: BuildInfo = make_build_info!();

const HTTP_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

pub const SYSLOG_IDENTIFIER: &str = "worldcoin-attest";

#[allow(clippy::missing_errors_doc)]
pub async fn main() -> eyre::Result<()> {
    info!("Version: {}", BUILD_INFO.version);

    let orb_id =
        std::env::var("ORB_ID").wrap_err("env variable `ORB_ID` should be set")?;
    let config = config::Config::new(config::Backend::new(), &orb_id);

    let force_refresh_token = Arc::new(Notify::new());

    let iface_ref = setup_dbus(force_refresh_token.clone())
        .await
        .wrap_err("Initialization failed")?;
    let conn = iface_ref.signal_context().connection().clone();
    let run_fut = run(
        &orb_id,
        iface_ref,
        force_refresh_token.clone(),
        config.auth_url,
        config.ping_url,
    );

    let mut msg_stream = zbus::MessageStream::from(conn);
    let dbus_monitor_task = tokio::spawn(async move {
        while let Some(_msg) = msg_stream.next().await {}
        bail!("Lost DBus connection")
    });

    let ((), ()) = tokio::try_join!(
        run_fut.map(|r| r.wrap_err("main task errored")),
        dbus_monitor_task
            .map(|r| r.wrap_err("dbus monitor task terminated abnormally")?)
    )?;
    Ok(())
}

/// Return either a *proovenly working* static token, or a short lived token.
#[tracing::instrument]
async fn get_working_token(
    orb_id: &str,
    auth_url: &Url,
    ping_url: &Url,
) -> crate::remote_api::Token {
    select! {
        Ok(token) = get_working_static_token(orb_id, ping_url) => token,
        token = remote_api::get_token(orb_id, auth_url) => token,
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
                let () = sleep(HTTP_RETRY_DELAY).await;
                continue;
            }
        }
    }
}

#[tracing::instrument]
async fn setup_dbus(
    force_refresh_token: Arc<Notify>,
) -> eyre::Result<zbus::InterfaceRef<crate::dbus::AuthTokenManagerIface>> {
    let dbus = dbus::create_dbus_connection(force_refresh_token)
        .await
        .wrap_err("failed to create DBus connection")?;

    let object_server = dbus.object_server();
    let iface_ref = object_server
        .interface::<_, dbus::AuthTokenManagerIface>("/org/worldcoin/AuthTokenManager1")
        .await
        .wrap_err("failed to get reference to AuthTokenManager1 from object server")?;

    Ok(iface_ref)
}

async fn run(
    orb_id: &str,
    iface_ref: zbus::InterfaceRef<dbus::AuthTokenManagerIface>,
    force_refresh_token: Arc<Notify>,
    auth_url: Url,
    ping_url: Url,
) -> eyre::Result<()> {
    loop {
        let token = get_working_token(orb_id, &auth_url, &ping_url).await;
        let token_refresh_delay = token.get_best_refresh_time();
        // get_mut() blocks access to the iface_ref object. So we never bind its result to be safe.
        // https://docs.rs/zbus/3.7.0/zbus/struct.InterfaceRef.html#method.get_mut
        iface_ref
            .get_mut()
            .await
            .0
            .update_token(token.token.expose_secret());
        iface_ref
            .get_mut()
            .await
            .token_changed(iface_ref.signal_context())
            .await
            .wrap_err("failed to send token_changed signal")?;

        //  Wait for whatever happens first: token expires or a refresh is requested
        select! {
            () = sleep(token_refresh_delay).fuse() => {info!("token is about to expire, refreshing it");},
            () = force_refresh_token.notified().fuse() => {info!("refresh was requested, refreshing the token");},
        };
    }
}
