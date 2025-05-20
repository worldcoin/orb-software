//! The update verifier crate provides methods to check the system health of the Orb.

use clap::Parser;
use color_eyre::eyre::Error;
use color_eyre::eyre::{self};
use eyre::{bail, eyre};
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::orb_os_release::OrbOsRelease;
use orb_slot_ctrl::program::Cli;
use orb_slot_ctrl::OrbSlotCtrl;
use std::process::Command;
use tracing::{error, info, instrument, warn};

#[allow(missing_docs)]
pub const BUILD_INFO: BuildInfo = make_build_info!();

#[instrument(err)]
pub fn run() -> eyre::Result<()> {
    let _args = Cli::parse();

    let os_release = OrbOsRelease::read_blocking()?;
    let orb_slot_ctrl = OrbSlotCtrl::new("/", os_release.orb_os_platform_type)?;

    let result = get_mcu_util_info()
        .and_then(|mcu_info| check_mcu_versions(&mcu_info, os_release));

    if let Err(e) = result {
        error!("Main MCU version check failed: {}", e);
        warn!("The main microcontroller might not be compatible, but is going to be used anyway.");
    } else {
        info!("mcu version check OK");
    }

    info!("Marking the current slot as OK");
    orb_slot_ctrl.mark_current_slot_ok()?;
    Ok(())
}

pub fn get_mcu_util_info() -> Result<String, Error> {
    // Get the current MCU version from orb-mcu-util
    let mcu_util_output = Command::new("orb-mcu-util")
        .arg("info")
        .output()
        .map_err(|e| eyre!("Failed to run orb-mcu-util: {e}"))?;

    if !mcu_util_output.status.success() {
        bail!(
            "orb-mcu-util failed: {}",
            String::from_utf8_lossy(&mcu_util_output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&mcu_util_output.stdout).to_string())
}
pub(crate) fn check_mcu_versions(
    stdout: &str,
    os_release: OrbOsRelease,
) -> eyre::Result<()> {
    let version_line = stdout
        .trim()
        .lines()
        .find(|line| line.trim_start().starts_with("current image:"));

    let current_version = if let Some(line) = version_line {
        line.split_whitespace()
            .nth(2)
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or("")
            .trim_start_matches('v')
    } else {
        bail!(
            "Could not parse MCU version from orb-mcu-util output: {}",
            stdout
        );
    };

    let expected_version = os_release.expected_main_mcu_version.trim_start_matches('v');

    if current_version.is_empty() {
        bail!("Current MCU version string is empty")
    }

    if expected_version.is_empty() {
        bail!("Expected MCU version string is empty");
    }

    if current_version != expected_version {
        bail!(
            "MCU version mismatch: found '{}', expected '{}'",
            current_version,
            expected_version
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orb_info::orb_os_release::{OrbReleaseType, OrbType};

    #[test]
    fn it_verifies_successfuly_mcu_is_correct_version() {
        let mcu_output = r"🔮 Orb info:^M
        revision:       EVT3^M
        battery charge: 82%^M
        voltage:        15956mV^M
        charging:       no^M
🚜 Main board:^M
        current image:  v2.2.4-0x2878a0fc (prod)^M
🔐 Security board:^M
        current image:  v1.0.7-0x4aed8dc1 (prod)^M
        secondary slot: v1.0.3-0xfa7b184d (prod)^M
        battery charge: 100%^M
        voltage:        4130mV^M
        charging:       no^M
        ";

        let os_release = OrbOsRelease {
            release_type: OrbReleaseType::Prod,
            orb_os_platform_type: OrbType::Pearl,
            expected_main_mcu_version: String::from("v2.2.4"),
            expected_sec_mcu_version: String::from("v1.0.3"),
        };

        let result = check_mcu_versions(mcu_output, os_release);
        assert!(result.is_ok(), "{:?}", result);
    }

    #[test]
    fn it_errors_if_mcu_util_info_is_empty() {}

    #[test]
    fn it_errors_if_sec_mcu_mismatches() {}

    #[test]
    fn it_errors_if_main_mcu_mismatches() {}

    #[test]
    fn it_errors_if_versions_are_unknown() {}
}
