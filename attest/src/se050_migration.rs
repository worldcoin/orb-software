use std::time::Duration;

use orb_info::OrbId;
use tracing::warn;
use url::Url;

use crate::remote_api::{self, MigratedKeyProbe};

const KEY_ACTIVATION_POLL_TIMEOUT: Duration = Duration::from_secs(60);
const KEY_ACTIVATION_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Poll after proof submission until the backend accepts the migrated key.
/// Only time spent receiving a definitive 403 counts toward the timeout;
/// communication failures keep retrying inside the probe.
async fn wait_for_migrated_key_activation(orb_id: &OrbId, auth_url: &Url) -> bool {
    let mut rejected_for = Duration::ZERO;

    while rejected_for < KEY_ACTIVATION_POLL_TIMEOUT {
        match remote_api::try_token_with_migrated_key(orb_id, auth_url).await {
            MigratedKeyProbe::Accepted => return true,
            MigratedKeyProbe::Inconclusive => {
                warn!(
                    "migrated key state remained inconclusive after proof submission; using legacy keys"
                );
                return false;
            }
            MigratedKeyProbe::BackendRejected => {
                tokio::time::sleep(KEY_ACTIVATION_POLL_INTERVAL).await;
                rejected_for += KEY_ACTIVATION_POLL_INTERVAL;
            }
        }
    }

    warn!(
        "backend continued to reject migrated keys after proof submission; using legacy keys"
    );
    false
}

/// Determine which key set `orb-sign-attestation` should use.
///
/// 1. Attempt a complete challenge → migrated sign → token round-trip.
/// 2. A valid token selects migrated keys. Only a token 403 starts proof submission.
/// 3. Submit the attested migrated keys, then poll for backend activation.
///
/// Backend and SE050 communication failures are retried by the probe and never
/// cause fallback to legacy keys.
pub async fn startup_key_selection(
    orb_id: &OrbId,
    auth_url: &Url,
    keys_challenge_url: &Url,
    keys_proof_url: &Url,
) -> bool {
    // First check whether the backend already accepts the migrated key.
    match remote_api::try_token_with_migrated_key(orb_id, auth_url).await {
        MigratedKeyProbe::Accepted => return true,
        MigratedKeyProbe::Inconclusive => {
            warn!("migrated key state is inconclusive; using legacy keys");
            return false;
        }
        MigratedKeyProbe::BackendRejected => {}
    }

    // A token 403 proved that the backend does not accept the migrated key yet.
    if let Err(error) =
        remote_api::submit_proof(orb_id, keys_challenge_url, keys_proof_url).await
    {
        warn!(%error, "migrated key proof submission failed; using legacy keys");
        return false;
    }

    // Proof submission succeeded; wait for the backend to activate the new key.
    wait_for_migrated_key_activation(orb_id, auth_url).await
}
