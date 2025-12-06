//! Verification commands that can be run on an Orb device over SSH.

use crate::ssh_wrapper::SshWrapper;
use color_eyre::{
    eyre::{ensure, eyre, WrapErr},
    Result,
};

/// Run the orb-update-verifier command on the Orb device.
pub async fn run_update_verifier(session: &SshWrapper) -> Result<String> {
    let result = session
        .execute_command("TERM=dumb sudo orb-update-verifier")
        .await
        .wrap_err("Failed to run orb-update-verifier")?;

    ensure!(
        result.is_success(),
        "orb-update-verifier failed: {}",
        result.stderr
    );

    Ok(result.stdout)
}

/// Get the capsule update status from nvbootctrl.
pub async fn get_capsule_update_status(session: &SshWrapper) -> Result<String> {
    let result = session
        .execute_command("TERM=dumb sudo nvbootctrl dump-slots-info")
        .await
        .wrap_err("Failed to get capsule update status")?;

    // Note: nvbootctrl returns exit code 1 with "Error: can not open /dev/mem" but still outputs valid info
    // So we don't check is_success() here, just parse the output

    parse_capsule_status_from_output(&result.stdout)
}

fn parse_capsule_status_from_output(output: &str) -> Result<String> {
    output
        .lines()
        .find(|line| line.starts_with("Capsule update status:"))
        .and_then(|line| line.split(':').nth(1).map(|s| s.trim().to_string()))
        .ok_or_else(|| {
            eyre!("Could not find 'Capsule update status' in nvbootctrl output")
        })
}

/// Run the check-my-orb command on the Orb device.
pub async fn run_check_my_orb(session: &SshWrapper) -> Result<String> {
    let result = session
        .execute_command("TERM=dumb check-my-orb")
        .await
        .wrap_err("Failed to run check-my-orb")?;

    if !result.is_success() {
        return Err(eyre!(
            "check-my-orb failed with exit code: {}",
            result.stderr
        ));
    }

    Ok(result.stdout)
}

pub async fn get_boot_time(session: &SshWrapper) -> Result<String> {
    let result = session
        .execute_command("TERM=dumb systemd-analyze time")
        .await
        .wrap_err("Failed to run systemd-analyze")?;

    ensure!(
        result.is_success(),
        "systemd-analyze failed: {}",
        result.stderr
    );

    Ok(result.stdout)
}
