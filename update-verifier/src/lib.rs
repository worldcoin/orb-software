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

    info!(
        "performing system health checks: rootfs status: {:?}, dry-run: {:?}",
        orb_slot_ctrl.get_current_rootfs_status()?,
        dry_run
    );

    // Check that the main microcontroller version is compatible with the
    // current firmware.
    match Mcu::main().run_check() {
        Ok(()) => {}
        Err(
            Error::RecoverableVersionMismatch(..) | Error::SecondaryIsMoreRecent(_),
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

    info!("system health is OK");

    info!("setting rootfs status to Normal");
    orb_slot_ctrl.set_current_rootfs_status(orb_slot_ctrl::RootFsStatus::Normal)?;

    // Set BootChainFwStatus to 0 to indicate successful update verification
    info!("setting BootChainFwStatus to 0 to indicate successful update verification");
    if let Err(e) = orb_slot_ctrl.set_fw_status(0) {
        error!("Failed to set BootChainFwStatus: {}", e);
    }

    // Reset PMC registers to prevent boot loop on Orin
    info!("Resetting PMC registers");
    reset_pmc_registers()?;

    Ok(())
}

/// Resets PMC registers to prevent boot loops on Orin platform.
///
/// # Errors
/// Returns error if unable to open or write to the PMC register files.
fn reset_pmc_registers() -> eyre::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let pmc_files = [
        "/sys/devices/platform/bus@0/c360000.pmc/rootfs_sr_magic",
        "/sys/devices/platform/bus@0/c360000.pmc/rootfs_retry_count_a",
        "/sys/devices/platform/bus@0/c360000.pmc/rootfs_retry_count_b",
    ];

    for file_path in pmc_files {
        info!("Resetting PMC register: {}", file_path);
        match OpenOptions::new().write(true).create(false).open(file_path) {
            Ok(mut file) => {
                match file.write_all(b"0x0") {
                    Ok(_) => info!("Successfully reset {}", file_path),
                    Err(e) => error!("Failed to write to {}: {}", file_path, e),
                }
            },
            Err(e) => error!("Failed to open {}: {}", file_path, e),
        }
    }

    Ok(())
}
