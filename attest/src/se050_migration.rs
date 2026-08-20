use tracing::{info, warn};
use url::Url;

use crate::remote_api;

/// How long to poll for migrated key activation after proof submission.
const KEY_ACTIVATION_POLL_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(60);
const KEY_ACTIVATION_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(5);
/// How long to wait between retries while waiting for the backend to become reachable.
const BACKEND_REACHABILITY_RETRY_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(5);

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
        let client = crate::client::create();
        match remote_api::Challenge::request(&client, orb_id, &tokenchallenge_url).await
        {
            Ok(_) | Err(remote_api::ChallengeError::ServerReturnedError(..)) => {
                info!("backend reachable");
                return;
            }
            Err(e) => {
                warn!(
                    "backend not reachable ({e}), retrying in {}s",
                    BACKEND_REACHABILITY_RETRY_INTERVAL.as_secs()
                );
                tokio::time::sleep(BACKEND_REACHABILITY_RETRY_INTERVAL).await;
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
pub async fn startup_key_selection(
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
        tokio::time::sleep(KEY_ACTIVATION_POLL_INTERVAL).await;
    }

    warn!(
        "backend did not accept migrated keys after {}s; proceeding with legacy keys",
        KEY_ACTIVATION_POLL_TIMEOUT.as_secs()
    );
    false
}
