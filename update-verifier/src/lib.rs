//! The update verifier crate provides methods to check the system health of the Orb.
#![warn(clippy::pedantic, missing_docs)]

use orb_build_info::{make_build_info, BuildInfo};
use orb_slot_ctrl::{EfiVarDb, OrbSlotCtrl};
use tracing::{error, info, instrument, warn};

use crate::checks::mcu::{Error, Mcu};
use crate::checks::teleport::Teleport;
use crate::checks::Check;

mod checks;

#[allow(missing_docs)]
pub const BUILD_INFO: BuildInfo = make_build_info!();

/// Performs the system health check.
///
/// # Errors
/// Can throw errors of `slot-ctrl` library or when calling system health checks.
#[instrument(err)]
pub fn run_health_check() -> eyre::Result<()> {
    // get runtime environment variable to force health check
    let dry_run = std::env::var("UPDATE_VERIFIER_DRY_RUN").is_ok();
    let orb_slot_ctrl = OrbSlotCtrl::new(&EfiVarDb::from_rootfs("/")?)?;

    if orb_slot_ctrl.get_current_rootfs_status()?.is_normal() && !dry_run {
        info!("skipping system health checks since rootfs status is Normal");
    } else {
        info!(
            "performing system health checks: rootfs status: {:?}, dry-run: {:?}",
            orb_slot_ctrl.get_current_rootfs_status()?,
            dry_run
        );

        // In case rootfs status is NOT Normal, and we know it's the first boot attempt
        // by checking the retry counter
        // we check that the main microcontroller version is compatible with the
        // current firmware and if not, we retry to apply the update once, and only once.
        // On any error, we skip the check
        if let (Ok(retry_count), Ok(max_retry_count)) = (
            orb_slot_ctrl.get_current_retry_count(),
            orb_slot_ctrl.get_max_retry_count(),
        ) {
            // ⚠️ retry counter already decremented once booted
            // use `>=` for testing purposes as the counter is reset to MAX
            // on each successful execution, but we might want to check the
            // health check logic multiple times
            if retry_count >= (max_retry_count - 1) {
                match Mcu::main().run_check() {
                    Ok(()) => {}
                    Err(
                        Error::RecoverableVersionMismatch(..)
                        | Error::SecondaryIsMoreRecent(_),
                    ) => {
                        info!("Activating and rebooting for mcu update retry");
                        if dry_run {
                            warn!("Dry-run: skipping mcu update retry");
                        } else {
                            Mcu::main().reboot_for_update()?;
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        error!("Main MCU version check failed: {}", e);
                        warn!("The main microcontroller might not be compatible, but is going to be used anyway.");
                    }
                }
            }
        } else {
            warn!("Could not get retry count or max retry count, skipping main MCU version check");
        }

        // check the teleport service health
        Teleport::default().run_check()?;

        info!("system health is OK");

        info!("setting rootfs status to Normal");
        orb_slot_ctrl.set_current_rootfs_status(orb_slot_ctrl::RootFsStatus::Normal)?;
    }

    info!("setting retry counter to maximum for future boot attempts");
    orb_slot_ctrl.reset_current_retry_count_to_max()?;
    Ok(())
}
