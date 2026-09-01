#![cfg_attr(not(test), forbid(unsafe_code))]

pub mod client;
pub mod config;
pub mod dbus;
pub mod remote_api;
#[cfg(feature = "se050_key_migration")]
mod se050_migration;

use eyre::{self, bail, WrapErr};
use futures::{FutureExt, StreamExt};
use orb_build_info::{make_build_info, BuildInfo};
use orb_dogd::{DogstatsdClient, MetricEmitter};
use orb_info::{
    orb_os_release::{OrbOsPlatform, OrbOsRelease},
    OrbId,
};
use prelude::connectivity::tracker::ConnectivityTracker;
use secrecy::ExposeSecret;
use std::default::Default;
use std::{sync::Arc, time::Duration};
use tokio::{select, sync::Notify, time::sleep};
use tracing::{info, warn};
use url::Url;
use zenorb::{zenoh::sample::Sample, Zenorb};

const BUILD_INFO: BuildInfo = make_build_info!();

const HTTP_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

/// Relative path of migrated iris-code public key blob.
const MIGRATED_IRIS_CODE_PUBKEY: &str = "sss_60000002_0002_0040.bin";
/// Relative path of legacy iris-code public key blob.
const LEGACY_IRIS_CODE_PUBKEY: &str = "sss_70000002_0002_0040.bin";

pub const SYSLOG_IDENTIFIER: &str = "worldcoin-attest";

#[allow(clippy::missing_errors_doc)]
pub async fn main() -> eyre::Result<()> {
    info!("Version: {}", BUILD_INFO.version);

    let orb_id = OrbId::read().await?;
    let platform = OrbOsRelease::read()
        .await
        .wrap_err("failed to determine Orb platform")?
        .orb_os_platform_type;
    let config = config::Config::new(config::default_backend(), orb_id.as_str());

    let force_refresh_token = Arc::new(Notify::new());

    let iface_ref = setup_dbus(force_refresh_token.clone())
        .await
        .wrap_err("Initialization failed")?;

    // SE050 key migration is Diamond-only; Pearl uses legacy keys.
    let new_keys_active = if platform == OrbOsPlatform::Pearl {
        false
    } else {
        startup_key_selection(
            orb_id.as_str(),
            &config.auth_url,
            &config.keys_challenge_url,
            &config.keys_proof_url,
        )
        .await
    };

    if new_keys_active {
        info!("using {MIGRATED_IRIS_CODE_PUBKEY} as a signup key");
    } else {
        info!("using {LEGACY_IRIS_CODE_PUBKEY} as a signup key");
    }

    iface_ref
        .get_mut()
        .await
        .0
        .set_new_keys_active(new_keys_active);
    iface_ref
        .get_mut()
        .await
        .new_keys_active_changed(iface_ref.signal_context())
        .await
        .wrap_err("failed to send new_keys_active_changed signal")?;

    let conn = iface_ref.signal_context().connection().clone();

    let connectivity_tracker = ConnectivityTracker::default();

    let _zenorb = match Zenorb::from_cfg(zenorb::default_cfg())
        .orb_id(orb_id.clone())
        .with_name("attest")
        .await
    {
        Ok(zenorb) => {
            let _ = zenorb
                .receiver(connectivity_tracker.clone())
                .querying_subscriber(
                    "connd/oes/active_connections",
                    Duration::from_secs(1),
                    update_connectivity_tracker,
                )
                .run()
                .await?;

            Some(zenorb)
        }

        Err(e) => {
            warn!("zenoh not available, connectivity tracking disabled: {e}");
            None
        }
    };

    let run_fut = run(
        orb_id.as_str(),
        iface_ref,
        force_refresh_token.clone(),
        config.auth_url,
        config.ping_url,
        connectivity_tracker,
        DogstatsdClient::default(),
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

#[cfg(feature = "se050_key_migration")]
use se050_migration::startup_key_selection;

/// SE050 key migration is compiled out: never attempt it, always use legacy keys.
#[cfg(not(feature = "se050_key_migration"))]
async fn startup_key_selection(
    _orb_id: &str,
    _auth_url: &Url,
    _keys_challenge_url: &Url,
    _keys_proof_url: &Url,
) -> bool {
    false
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
        let client = client::create();
        match client::validate_token(&client, orb_id, &token, ping_url).await {
            Ok(true) => {
                info!("Static token is valid");
                return Ok(token);
            }
            // TODO make this error more specific
            Ok(false) => {
                return Err(std::io::Error::other("token was rejected by the backend"));
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
    conn_tracker: ConnectivityTracker,
    metrics: impl MetricEmitter,
) -> eyre::Result<()> {
    loop {
        let token = select! {
            Ok(token) = get_working_static_token(orb_id, &ping_url) => token,
            token = remote_api::get_token(orb_id, &auth_url, &metrics, &conn_tracker) => token,
        };

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
            () = sleep(token_refresh_delay).fuse() => {
                info!("token is about to expire, refreshing it");
            },

            () = force_refresh_token.notified().fuse() => {
                info!("refresh was requested, refreshing the token");
            },
        };
    }
}

async fn update_connectivity_tracker(
    conn_tracker: ConnectivityTracker,
    sample: Sample,
) -> color_eyre::Result<()> {
    let active_conns: oes::ActiveConnections =
        serde_json::from_slice(&sample.payload().to_bytes())
            .context("failed to parse ActiveConnections json")?;

    let has_internet = active_conns.connections.iter().any(|c| c.has_internet);
    conn_tracker.update(has_internet);

    Ok(())
}
