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
    use tracing::{info, warn};

    const MAX_ATTEMPTS: u32 = 60; // 60 attempts = 2 minutes max wait
    const SLEEP_DURATION: Duration = Duration::from_secs(2);
    // Timeout for individual command execution (10 seconds is generous for timedatectl/chronyc)
    const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

    info!("Waiting for system time synchronization...");
    let sync_start = std::time::Instant::now();

    // Detect which time sync tool is available (prefer chronyc over timedatectl)
    let use_chronyc = session
        .execute_command("TERM=dumb command -v chronyc")
        .await
        .map(|r| r.is_success())
        .unwrap_or(false);

    let use_timedatectl = if !use_chronyc {
        session
            .execute_command("TERM=dumb command -v timedatectl")
            .await
            .map(|r| r.is_success())
            .unwrap_or(false)
    } else {
        false
    };

    if !use_timedatectl && !use_chronyc {
        bail!("Neither chronyc nor timedatectl found on the system");
    }

    info!(
        "Using {} for time sync check",
        if use_chronyc {
            "chronyc"
        } else {
            "timedatectl"
        }
    );

    for attempt in 1..=MAX_ATTEMPTS {
        let is_synced = if use_chronyc {
            // Try chronyc tracking with timeout
            match tokio::time::timeout(
                COMMAND_TIMEOUT,
                session.execute_command("TERM=dumb chronyc tracking"),
            )
            .await
            {
                Ok(Ok(result)) if result.is_success() => {
                    // Check if chrony is synchronized
                    // Leap status should be "Normal" when synchronized
                    result.stdout.contains("Leap status     : Normal")
                        && !result.stdout.contains("Reference ID    : 0.0.0.0")
                }
                Ok(Ok(_)) => false,
                Ok(Err(e)) => {
                    info!(
                        "Failed to check chronyc status (attempt {}/{}): {}",
                        attempt, MAX_ATTEMPTS, e
                    );
                    false
                }
                Err(_) => {
                    info!(
                        "chronyc command timed out after {:?} (attempt {}/{})",
                        COMMAND_TIMEOUT, attempt, MAX_ATTEMPTS
                    );
                    false
                }
            }
        } else {
            // Try timedatectl with timeout
            match tokio::time::timeout(
                COMMAND_TIMEOUT,
                session.execute_command("TERM=dumb timedatectl"),
            )
            .await
            {
                Ok(Ok(result)) if result.is_success() => {
                    // Check if "System clock synchronized: yes" appears in output
                    result.stdout.contains("System clock synchronized: yes")
                        || result.stdout.contains("synchronized: yes")
                }
                Ok(Ok(_)) => false,
                Ok(Err(e)) => {
                    info!(
                        "Failed to check timedatectl status (attempt {}/{}): {}",
                        attempt, MAX_ATTEMPTS, e
                    );
                    false
                }
                Err(_) => {
                    info!(
                        "timedatectl command timed out after {:?} (attempt {}/{})",
                        COMMAND_TIMEOUT, attempt, MAX_ATTEMPTS
                    );
                    false
                }
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

    warn!(
        "timedatectl did not report sync after {} seconds, falling back to date comparison",
        MAX_ATTEMPTS * 2
    );

    // Fallback: Compare Orb's date with PC's date
    // If difference is less than 1 month, consider it acceptable
    check_time_difference_fallback(session).await
}

/// Fallback time check: Compare Orb's time with local PC time
/// Accept if difference is less than 1 month
async fn check_time_difference_fallback(session: &SshWrapper) -> Result<()> {
    use tracing::info;

    info!("Checking time difference between Orb and local PC...");

    // Get Orb's current timestamp (Unix epoch seconds)
    let orb_time_result = session
        .execute_command("TERM=dumb date +%s")
        .await
        .wrap_err("Failed to get Orb's timestamp")?;

    ensure!(
        orb_time_result.is_success(),
        "Failed to get Orb timestamp: {}",
        orb_time_result.stderr
    );

    let orb_timestamp: i64 = orb_time_result
        .stdout
        .trim()
        .parse()
        .wrap_err("Failed to parse Orb timestamp")?;

    // Get local PC's current timestamp
    let local_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .wrap_err("Failed to get local system time")?
        .as_secs() as i64;

    let time_diff_seconds = (orb_timestamp - local_timestamp).abs();
    let time_diff_days = time_diff_seconds / 86400; // 86400 seconds in a day

    const MAX_ACCEPTABLE_DIFF_DAYS: i64 = 30; // 1 month tolerance

    info!(
        "Time difference: {} days ({} seconds)",
        time_diff_days, time_diff_seconds
    );

    if time_diff_seconds < MAX_ACCEPTABLE_DIFF_DAYS * 86400 {
        info!(
            "Time difference of {} days is within acceptable range (< {} days)",
            time_diff_days, MAX_ACCEPTABLE_DIFF_DAYS
        );

        Ok(())
    } else {
        bail!(
            "Time difference too large: {} days (max acceptable: {} days). \
             Orb time may be significantly out of sync.",
            time_diff_days,
            MAX_ACCEPTABLE_DIFF_DAYS
        );
    }
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
