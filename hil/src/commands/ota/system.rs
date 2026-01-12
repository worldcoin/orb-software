use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result,
};
use orb_hil::SshWrapper;
use serde_json::Value;

/// Reboot the Orb device using orb-mcu-util and shutdown
pub async fn reboot_orb(session: &SshWrapper) -> Result<()> {
    session
        .execute_command("TERM=dumb orb-mcu-util reboot orb")
        .await
        .wrap_err("Failed to execute orb-mcu-util reboot orb")?;

    session
        .execute_command("TERM=dumb sudo shutdown now")
        .await
        .wrap_err("Failed to execute shutdown now")?;

    Ok(())
}

/// Wipe overlays on the device (Diamond platform specific)
pub async fn wipe_overlays(session: &SshWrapper) -> Result<()> {
    let result = session
        .execute_command("bash -c 'source ~/.bash_profile 2>/dev/null || true; source ~/.bashrc 2>/dev/null || true; wipe_overlays'")
        .await
        .wrap_err("Failed to execute wipe_overlays function")?;

    ensure!(
        result.is_success(),
        "wipe_overlays function failed: {}",
        result.stderr
    );

    Ok(())
}

/// Get the current boot slot (a or b)
pub async fn get_current_slot(session: &SshWrapper) -> Result<String> {
    let result = session
        .execute_command("TERM=dumb orb-slot-ctrl -c")
        .await
        .wrap_err("Failed to execute orb-slot-ctrl -c")?;

    ensure!(
        result.is_success(),
        "orb-slot-ctrl -c failed: {}",
        result.stderr
    );

    parse_slot_from_output(&result.stdout)
}

/// Parse slot letter from orb-slot-ctrl output
fn parse_slot_from_output(output: &str) -> Result<String> {
    let slot_letter = if output.contains('a') {
        'a'
    } else if output.contains('b') {
        'b'
    } else {
        bail!("Could not parse current slot from: {}", output);
    };

    Ok(format!("slot_{slot_letter}"))
}

/// Update versions.json with the target version for the given slot
pub async fn update_versions_json(
    session: &SshWrapper,
    current_slot: &str,
    target_version: &str,
) -> Result<()> {
    let result = session
        .execute_command("TERM=dumb cat /usr/persistent/versions.json")
        .await
        .wrap_err("Failed to read /usr/persistent/versions.json")?;

    ensure!(
        result.is_success(),
        "Failed to read versions.json: {}",
        result.stderr
    );

    let updated_json =
        update_versions_json_content(&result.stdout, current_slot, target_version)?;

    let result = session
        .execute_command(&format!(
            "echo '{updated_json}' | sudo tee /usr/persistent/versions.json > /dev/null"
        ))
        .await
        .wrap_err("Failed to write updated versions.json")?;

    ensure!(
        result.is_success(),
        "Failed to write versions.json: {}",
        result.stderr
    );

    Ok(())
}

/// Pure function to update versions.json content
fn update_versions_json_content(
    json_content: &str,
    current_slot: &str,
    target_version: &str,
) -> Result<String> {
    let mut versions_data: Value =
        serde_json::from_str(json_content).wrap_err("Failed to parse versions.json")?;

    let releases = versions_data.get_mut("releases").ok_or_else(|| {
        color_eyre::eyre::eyre!("releases field not found in versions.json")
    })?;

    let releases_obj = releases.as_object_mut().ok_or_else(|| {
        color_eyre::eyre::eyre!("releases field is not an object in versions.json")
    })?;

    releases_obj.insert(
        current_slot.to_string(),
        Value::String(target_version.to_string()),
    );

    serde_json::to_string_pretty(&versions_data)
        .wrap_err("Failed to serialize updated versions.json")
}

/// Wait for system time to be synchronized via NTP/chrony
pub async fn wait_for_time_sync(session: &SshWrapper) -> Result<()> {
    use std::time::Duration;
    use tracing::info;

    const MAX_ATTEMPTS: u32 = 60; // 60 attempts = 2 minutes max wait
    const SLEEP_DURATION: Duration = Duration::from_secs(2);
    // Timeout for individual command execution (10 seconds is generous for timedatectl/chronyc)
    const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

    info!("Waiting for system time synchronization...");
    let sync_start = std::time::Instant::now();

    // Detect which time sync tool is available
    let use_timedatectl = session
        .execute_command("TERM=dumb command -v timedatectl")
        .await
        .map(|r| r.is_success())
        .unwrap_or(false);

    let use_chronyc = session
        .execute_command("TERM=dumb command -v chronyc")
        .await
        .map(|r| r.is_success())
        .unwrap_or(false);

    if !use_timedatectl && !use_chronyc {
        bail!("Neither timedatectl nor chronyc found on the system");
    }

    info!(
        "Using {} for time sync check",
        if use_timedatectl {
            "timedatectl"
        } else {
            "chronyc"
        }
    );

    for attempt in 1..=MAX_ATTEMPTS {

        let is_synced = if use_timedatectl {
            // Try timedatectl with timeout
            let result = tokio::time::timeout(
                COMMAND_TIMEOUT,
                session.execute_command("TERM=dumb timedatectl"),
            )
            .await
            .map_err(|_| {
                color_eyre::eyre::eyre!(
                    "timedatectl command timed out after {:?}",
                    COMMAND_TIMEOUT
                )
            })?
            .wrap_err("Failed to check time synchronization status")?;

            if result.is_success() {
                // Check if "System clock synchronized: yes" appears in output
                result.stdout.contains("System clock synchronized: yes")
                    || result.stdout.contains("synchronized: yes")
            } else {
                false
            }
        } else {
            // Try chronyc tracking with timeout
            let result = tokio::time::timeout(
                COMMAND_TIMEOUT,
                session.execute_command("TERM=dumb chronyc tracking"),
            )
            .await
            .map_err(|_| {
                color_eyre::eyre::eyre!(
                    "chronyc command timed out after {:?}",
                    COMMAND_TIMEOUT
                )
            })?
            .wrap_err("Failed to check time synchronization status")?;

            if result.is_success() {
                // Check if chrony is synchronized
                // Leap status should be "Normal" when synchronized
                result.stdout.contains("Leap status     : Normal")
                    && !result.stdout.contains("Reference ID    : 0.0.0.0")
            } else {
                false
            }
        };

        if is_synced {
            let sync_duration = sync_start.elapsed();
            info!(
                "System time synchronized successfully after {:?}",
                sync_duration
            );
            return Ok(());
        }

        if attempt < MAX_ATTEMPTS {
            info!(
                "Time not yet synchronized (attempt {}/{}), waiting...",
                attempt, MAX_ATTEMPTS
            );
            tokio::time::sleep(SLEEP_DURATION).await;
        }
    }

    bail!(
        "Timeout waiting for system time synchronization after {} seconds",
        MAX_ATTEMPTS * 2
    );
}

/// Restart the update agent service and return the start timestamp
pub async fn restart_update_agent(session: &SshWrapper) -> Result<String> {
    // Get current timestamp (ON THE ORB!) before restarting service
    let timestamp_result = session
        .execute_command("TERM=dumb date '+%Y-%m-%d %H:%M:%S'")
        .await
        .wrap_err("Failed to get current timestamp")?;

    ensure!(
        timestamp_result.is_success(),
        "Failed to get timestamp: {}",
        timestamp_result.stderr
    );

    let start_timestamp = timestamp_result.stdout.trim().to_string();

    let result = session
        .execute_command(
            "TERM=dumb sudo systemctl restart worldcoin-update-agent.service",
        )
        .await
        .wrap_err("Failed to restart worldcoin-update-agent.service")?;

    ensure!(
        result.is_success(),
        "Failed to restart worldcoin-update-agent.service: {}",
        result.stderr
    );

    Ok(start_timestamp)
}
