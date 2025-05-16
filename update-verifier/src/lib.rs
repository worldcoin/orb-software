//! The update verifier crate provides methods to check the system health of the Orb.
#![warn(clippy::pedantic, missing_docs)]

use crate::checks::mcu::{Error, Mcu};
use crate::checks::Check;
use color_eyre::eyre;
use orb_build_info::{make_build_info, BuildInfo};
use orb_slot_ctrl::OrbSlotCtrl;
use tracing::{error, info, instrument, warn};

mod checks;

#[allow(missing_docs)]
pub const BUILD_INFO: BuildInfo = make_build_info!();

/// Performs the system health check.
///
/// # Errors
/// Can throw errors of `slot-ctrl` library or when calling system health checks.
#[instrument(err, skip(orb_slot_ctrl))]
pub fn run_health_check(orb_slot_ctrl: OrbSlotCtrl) -> eyre::Result<()> {
    // get runtime environment variable to force health check
    let dry_run = std::env::var("UPDATE_VERIFIER_DRY_RUN").is_ok();

    if orb_slot_ctrl.get_current_rootfs_status()?.is_normal() && !dry_run {
        info!("skipping system health checks since rootfs status is Normal");
    } else {
        info!(
            "performing system health checks: rootfs status: {:?}, dry-run: {:?}",
            orb_slot_ctrl.get_current_rootfs_status()?,
            dry_run
        );

        // TODO:
        // Removed the retry-count logic and @vmenege will reason about this in the review
        // Switched to using a new, simplified version check.
        // run_check() now calls self.check_current()
        match Mcu::main().run_check() {
            Ok(()) => {}
            Err(e) => {
                // TODO: DataDog here ?
                error!("Main MCU version check failed: {}", e);
                warn!("The main microcontroller might not be compatible, but is going to be used anyway.");
            }
        }
        // Is it? What if MCU and OS are not compatable and we set the retry count to max later,
        // which might create infinite boot loop.
        info!("system health is OK");

        info!("setting rootfs status to Normal");
        orb_slot_ctrl.set_current_rootfs_status(orb_slot_ctrl::RootFsStatus::Normal)?;
    }

    info!("setting retry counter to maximum for future boot attempts");
    orb_slot_ctrl.reset_current_retry_count_to_max()?;
    Ok(())
}
