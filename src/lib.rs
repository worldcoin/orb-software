//! The update verifier crate provides methods to check the system health of the Orb.
#![warn(clippy::pedantic, missing_docs)]

mod checks;

use crate::checks::teleport::Teleport;
use crate::checks::Check;
use tracing::{info, instrument};

/// Performs the system health check.
///
/// # Errors
/// Can throw errors of `slot-ctrl` library or when calling system health checks.
#[instrument(err)]
pub fn run_health_check() -> eyre::Result<()> {
    if slot_ctrl::get_current_rootfs_status()?.is_normal() {
        info!("skipping system health checks since rootfs status is Normal");
    } else {
        info!("performing system health checks on rootfs status other than Normal");

        Teleport::default().run_check()?;

        info!("system health is OK");

        info!("setting rootfs status to Normal");
        slot_ctrl::set_current_rootfs_status(slot_ctrl::RootFsStatus::Normal)?;
    }

    info!("setting retry counter to maximum for future boot attempts");
    slot_ctrl::reset_current_retry_count_to_max()?;
    Ok(())
}
