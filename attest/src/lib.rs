#![cfg_attr(not(test), forbid(unsafe_code))]

pub mod client;
pub mod config;
pub mod dbus;
pub mod remote_api;

use eyre::{self, bail, WrapErr};
use futures::{FutureExt, StreamExt};
use orb_build_info::{make_build_info, BuildInfo};
use orb_dogd::{DogstatsdClient, MetricEmitter};
use orb_info::OrbId;
use secrecy::ExposeSecret;
use std::default::Default;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    select,
    sync::{watch, Notify},
    task::{self, JoinHandle},
    time::sleep,
};
use tracing::{info, warn};
use url::Url;
use zenorb::{zenoh::sample::Sample, Zenorb};

const BUILD_INFO: BuildInfo = make_build_info!();

const HTTP_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

/// How long to poll for migrated key activation after proof submission.
const KEY_ACTIVATION_POLL_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(60);
const KEY_ACTIVATION_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(5);
/// How long to wait between retries while waiting for the backend to become reachable.
const BACKEND_REACHABILITY_RETRY_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(5);

/// Relative path of migrated iris-code public key blob.
const MIGRATED_IRIS_CODE_PUBKEY: &str = "sss_60000002_0002_0040.bin";
/// Relative path of legacy iris-code public key blob.
const LEGACY_IRIS_CODE_PUBKEY: &str = "sss_70000002_0002_0040.bin";

pub const SYSLOG_IDENTIFIER: &str = "worldcoin-attest";

#[allow(clippy::missing_errors_doc)]
pub async fn main() -> eyre::Result<()> {
    info!("Version: {}", BUILD_INFO.version);

    let orb_id = OrbId::read().await?;
    let config = config::Config::new(config::default_backend(), orb_id.as_str());

    let force_refresh_token = Arc::new(Notify::new());

    let iface_ref = setup_dbus(force_refresh_token.clone())
        .await
        .wrap_err("Initialization failed")?;

    // Determine which SE050 key set is active before starting the token loop.
    let new_keys_active = startup_key_selection(
        orb_id.as_str(),
        &config.auth_url,
        &config.keys_challenge_url,
        &config.keys_proof_url,
    )
    .await;

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

    let (is_online_tx, is_online_rx) = watch::channel(false);

    match Zenorb::from_cfg(zenorb::default_cfg())
        .orb_id(orb_id.clone())
        .with_name("attest")
        .await
    {
        Ok(zenorb) => {
            let _ = zenorb
                .receiver(is_online_tx)
                .querying_subscriber(
                    "connd/oes/active_connections",
                    Duration::from_secs(1),
                    update_is_online,
                )
                .run()
                .await?;
        }
        Err(e) => {
            warn!("zenoh not available, connectivity tracking disabled: {e}");
        }
    }

    let run_fut = run(
        orb_id.as_str(),
        iface_ref,
        force_refresh_token.clone(),
        config.auth_url,
        config.ping_url,
        is_online_rx,
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

/// Returns `true` if `worldcoin-se050-provision.service` is in the `active`
/// state, meaning SE050 has migrated keys.
async fn se050_provision_service_active() -> bool {
    use zbus_systemd::systemd1::{ManagerProxy, UnitProxy};

    let Ok(conn) = zbus::Connection::system().await else {
        warn!("SE050: failed to connect to system D-Bus");
        return false;
    };
    let Ok(manager) = ManagerProxy::new(&conn).await else {
        warn!("SE050: failed to create systemd ManagerProxy");
        return false;
    };
    // GetUnit errors if the unit has never been loaded — treat as not active.
    let Ok(unit_path) = manager
        .get_unit("worldcoin-se050-provision.service".to_owned())
        .await
    else {
        warn!("SE050: worldcoin-se050-provision.service not found in systemd");
        return false;
    };
    let builder = match UnitProxy::builder(&conn).path(unit_path) {
        Ok(b) => b,
        Err(e) => {
            warn!("SE050: invalid unit object path from systemd: {e}");
            return false;
        }
    };
    let Ok(unit) = builder.build().await else {
        warn!("SE050: failed to create systemd UnitProxy");
        return false;
    };
    unit.active_state().await.ok().as_deref() == Some("active")
}

/// Block until the token-challenge endpoint returns any response (success or
/// server error). Retries indefinitely on network/connect errors — the caller
/// cannot do anything useful without backend connectivity.
async fn wait_for_backend_reachable(orb_id: &str, auth_url: &Url) {
    let tokenchallenge_url = auth_url
        .join("tokenchallenge")
        .expect("auth_url must have a path");
    loop {
        match remote_api::Challenge::request(orb_id, &tokenchallenge_url).await {
            Ok(_) | Err(remote_api::ChallengeError::ServerReturnedError(..)) => {
                info!("backend reachable");
                return;
            }
            Err(e) => {
                warn!(
                    "backend not reachable ({e}), retrying in {}s",
                    BACKEND_REACHABILITY_RETRY_INTERVAL.as_secs()
                );
                sleep(BACKEND_REACHABILITY_RETRY_INTERVAL).await;
            }
        }
    }
}

/// Determine which SE050 key set is active.
///
/// 1. Service check: if `worldcoin-se050-provision.service` is not active,
///    SE050 has no migrated keys → return `false` immediately.
/// 2. Wait until the backend is reachable (challenge endpoint responds).
/// 3. Backend check: attempt a full challenge→sign(migrated)→token round-trip.
///    If the backend returns a valid token, it already has the migrated key
///    registered → return `true`.
/// 4. Submit the NXP-attested proof so the backend registers the migrated key.
/// 5. Poll the same backend round-trip until it succeeds or [`KEY_ACTIVATION_POLL_TIMEOUT`]
///    elapses. "Activation" means the backend accepted the migrated key and
///    returned a valid token.
async fn startup_key_selection(
    orb_id: &str,
    auth_url: &Url,
    keys_challenge_url: &Url,
    keys_proof_url: &Url,
) -> bool {
    // Service check: worldcoin-se050-provision.service encodes the answer to
    // "does this Orb have migrated keys?" — it runs before us and stays active.
    if !se050_provision_service_active().await {
        info!("worldcoin-se050-provision.service not active, using legacy keys");
        return false;
    }

    // TODO: drop when other parts are ready, ORBS-1618, ORBS-1620
    if cfg!(not(feature = "se050_key_migration")) {
        warn!("skipping se050 migration due to feature flag");
        return false;
    }

    // Wait for connectivity: ensures the 60 s activation window is spent on
    // actual key-state probes, not wasted on network unreachability.
    wait_for_backend_reachable(orb_id, auth_url).await;

    // Backend check: does the backend already accept the migrated key?
    if remote_api::try_token_with_migrated_key(orb_id, auth_url).await {
        info!("backend already accepts migrated keys");
        return true;
    }

    // Submit proof to register the migrated public key with the backend.
    info!("migrated keys not yet accepted by backend, submitting key proof");
    match remote_api::submit_proof(orb_id, keys_challenge_url, keys_proof_url).await {
        Ok(()) => {
            info!(
                "key proof submitted, polling for backend activation (up to {}s)",
                KEY_ACTIVATION_POLL_TIMEOUT.as_secs()
            );
        }
        Err(e) => {
            warn!("key proof submission failed: {e}; proceeding with legacy keys");
            return false;
        }
    }

    // Poll until the backend accepts a token signed with the migrated key.
    // This is the definitive test: if the token endpoint returns 200, the
    // backend has swapped the registered public key and migration is complete.
    let deadline = tokio::time::Instant::now() + KEY_ACTIVATION_POLL_TIMEOUT;
    loop {
        if remote_api::try_token_with_migrated_key(orb_id, auth_url).await {
            info!("backend now accepts migrated keys");
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        sleep(KEY_ACTIVATION_POLL_INTERVAL).await;
    }

    warn!(
        "backend did not accept migrated keys after {}s; proceeding with legacy keys",
        KEY_ACTIVATION_POLL_TIMEOUT.as_secs()
    );
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
        match crate::client::validate_token(orb_id, &token, ping_url).await {
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
    is_online_rx: watch::Receiver<bool>,
    metrics: impl MetricEmitter,
) -> eyre::Result<()> {
    loop {
        let token = select! {
            Ok(token) = get_working_static_token(orb_id, &ping_url) => token,
            token = remote_api::get_token(orb_id, &auth_url, &metrics, &is_online_rx) => token,
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

async fn update_is_online(
    is_online: watch::Sender<bool>,
    sample: Sample,
) -> color_eyre::Result<()> {
    let active_conns: oes::ActiveConnections =
        serde_json::from_slice(&sample.payload().to_bytes())
            .context("failed to parse ActiveConnections json")?;

    let has_internet = active_conns.connections.iter().any(|c| c.has_internet);
    is_online
        .send(has_internet)
        .context("failed to send is_online watch value")?;

    Ok(())
}

pub struct ConnectivityTracker {
    stable_connection: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl Drop for ConnectivityTracker {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl ConnectivityTracker {
    pub fn start(mut is_online_rx: watch::Receiver<bool>) -> Self {
        let stable_connection = Arc::new(AtomicBool::new(*is_online_rx.borrow()));
        let sc = stable_connection.clone();

        let handle = task::spawn(async move {
            while let Ok(()) = is_online_rx.changed().await {
                if !*is_online_rx.borrow() {
                    sc.store(false, Ordering::Release);
                }
            }
        });

        Self {
            stable_connection,
            handle,
        }
    }

    /// Returns true if internet connection was stable (did not lose connection) for the lifetime of
    /// this ConnectivityTracker
    pub fn stable_connection(&self) -> bool {
        self.stable_connection.load(Ordering::Acquire)
    }
}
