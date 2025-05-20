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
    let main_version_line = stdout
        .trim()
        .lines()
        .find(|line| line.trim_start().starts_with("current image:"))
        .ok_or_else(|| eyre!("Could not find main MCU version line in output"))?;

    let sec_version_line = stdout
        .trim()
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with("current image:"))
        .ok_or_else(|| eyre!("Could not find security MCU version line in output"))?;

    let extract_version = |line: &str| {
        line.split_whitespace()
            .nth(2)
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string()
    };

    let current_main = extract_version(main_version_line);
    let current_sec = extract_version(sec_version_line);

    let expected_main = os_release.expected_main_mcu_version.trim_start_matches('v');
    let expected_sec = os_release.expected_sec_mcu_version.trim_start_matches('v');

    if current_main.is_empty() || expected_main.is_empty() {
        bail!("Main MCU version string is empty");
    }
    if current_sec.is_empty() || expected_sec.is_empty() {
        bail!("Secondary MCU version string is empty");
    }

    if current_main != expected_main {
        bail!(
            "Main MCU version mismatch: found '{}', expected '{}'",
            current_main,
            expected_main
        );
    }

    if current_sec != expected_sec {
        bail!(
            "Secondary MCU version mismatch: found '{}', expected '{}'",
            current_sec,
            expected_sec
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orb_info::orb_os_release::{OrbReleaseType, OrbType};

    fn generate_mcu_util_output_string(
        main_version: &str,
        sec_version: &str,
    ) -> String {
        format!(
            r"🔮 Orb info:^M
revision:       EVT3^M
battery charge: 82%^M
voltage:        15956mV^M
charging:       no^M
🚜 Main board:^M
        current image:  v{}-0x12345678 (prod)^M
🔐 Security board:^M
        current image:  v{}-0x87654321 (prod)^M
        secondary slot: v0.0.0-0x00000000 (prod)^M
        battery charge: 100%^M
        voltage:        4130mV^M
        charging:       no^M",
            main_version, sec_version
        )
    }

    #[test]
    fn it_verifies_successfully_mcu_is_correct_version() {
        let mcu_output = generate_mcu_util_output_string("2.2.4", "1.0.3");

        let os_release = OrbOsRelease {
            release_type: OrbReleaseType::Prod,
            orb_os_platform_type: OrbType::Pearl,
            expected_main_mcu_version: "v2.2.4".into(),
            expected_sec_mcu_version: "v1.0.3".into(),
        };

        let result = check_mcu_versions(&mcu_output, os_release);
        assert!(result.is_ok(), "{:?}", result);
    }

    #[test]
    fn it_errors_if_mcu_util_info_is_empty() {
        let mcu_output = "";

        let os_release = OrbOsRelease {
            release_type: OrbReleaseType::Prod,
            orb_os_platform_type: OrbType::Pearl,
            expected_main_mcu_version: "v2.2.4".into(),
            expected_sec_mcu_version: "v1.0.3".into(),
        };

        let result = check_mcu_versions(mcu_output, os_release);
        assert!(result.is_err());
    }

    #[test]
    fn it_errors_if_main_mcu_mismatches() {
        let mcu_output = generate_mcu_util_output_string("9.9.9", "1.0.3");

        let os_release = OrbOsRelease {
            release_type: OrbReleaseType::Prod,
            orb_os_platform_type: OrbType::Pearl,
            expected_main_mcu_version: "v2.2.4".into(),
            expected_sec_mcu_version: "v1.0.3".into(),
        };

        let result = check_mcu_versions(&mcu_output, os_release);
        assert!(result.is_err());
        assert!(
            format!("{:?}", result.unwrap_err()).contains("Main MCU version mismatch")
        );
    }

    #[test]
    fn it_errors_if_sec_mcu_mismatches() {
        let mcu_output = generate_mcu_util_output_string("2.2.4", "9.9.9");

        let os_release = OrbOsRelease {
            release_type: OrbReleaseType::Prod,
            orb_os_platform_type: OrbType::Pearl,
            expected_main_mcu_version: "v2.2.4".into(),
            expected_sec_mcu_version: "v1.0.3".into(),
        };

        let result = check_mcu_versions(&mcu_output, os_release);
        assert!(result.is_err());
        assert!(format!("{:?}", result.unwrap_err())
            .contains("Secondary MCU version mismatch"));
    }

    #[test]
    fn it_errors_if_versions_are_unknown() {
        let mcu_output = r"invalid mcu output with no version info";

        let os_release = OrbOsRelease {
            release_type: OrbReleaseType::Prod,
            orb_os_platform_type: OrbType::Pearl,
            expected_main_mcu_version: "v2.2.4".into(),
            expected_sec_mcu_version: "v1.0.3".into(),
        };

        let result = check_mcu_versions(mcu_output, os_release);
        assert!(result.is_err());
    }
}
