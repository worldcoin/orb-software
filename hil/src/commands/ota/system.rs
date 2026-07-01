use crate::remote_cmd::RemoteSession;
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result,
};

/// Reboot the Orb device using orb-mcu-util and shutdown
pub async fn reboot_orb(session: &RemoteSession) -> Result<()> {
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
pub async fn wipe_overlays(session: &RemoteSession) -> Result<()> {
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
pub async fn get_current_slot(session: &RemoteSession) -> Result<String> {
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

/// Kick off the update-agent flow for OTA.
pub async fn kickoff_update_agent_for_ota(
    session: &RemoteSession,
    target_version: &str,
    orb_id: &str,
    orb_ip: &str,
) -> Result<String> {
    let start_timestamp = get_current_timestamp(session).await?;

    let payload = serde_json::json!({
        "version": target_version.trim(),
        "restart": true,
    })
    .to_string();

    let output = tokio::process::Command::new("zorb")
        .args(["-o", orb_id, "-r", orb_ip])
        .args(["query", "supervisor/job/gondor", &payload])
        .output()
        .await
        .wrap_err("failed to spawn zorb on the runner")?;

    ensure!(
        output.status.success(),
        "failed to trigger OTA: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );

    Ok(start_timestamp)
}

/// Wait for system time to be synchronized via NTP/chrony
pub async fn wait_for_time_sync(session: &RemoteSession) -> Result<()> {
    use std::time::Duration;
    use tracing::info;

    const MAX_ATTEMPTS: u32 = 60; // 60 attempts = 2 minutes max wait
    const SLEEP_DURATION: Duration = Duration::from_secs(2);

    info!("Waiting for system time synchronization...");
    let sync_start = std::time::Instant::now();

    for attempt in 1..=MAX_ATTEMPTS {
        let result = session
            .execute_command("TERM=dumb chronyc tracking")
            .await
            .wrap_err("Failed to check time synchronization status")?;

        if result.is_success()
            && let Some(ref_id) = parse_chrony_reference_id(&result.stdout)
        {
            let sync_duration = sync_start.elapsed();
            info!(
                "System time synchronized (ref: {}) after {:?}",
                ref_id, sync_duration
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

/// Parse the Reference ID from `chronyc tracking` output.
///
/// Returns `Some(id_str)` when chrony has a valid NTP source (non-zero hex ID),
/// or `None` when unsynchronized (`00000000`).
///
/// Example output when synchronized:
/// ```text
/// Reference ID    : C035676C (ptbtime1.ptb.de)
/// Stratum         : 2
/// Ref time (UTC)  : Wed Apr 22 15:35:57 2026
/// System time     : 0.000264906 seconds fast of NTP time
/// Last offset     : +0.000107565 seconds
/// RMS offset      : 0.005323386 seconds
/// Frequency       : 22.376 ppm slow
/// Residual freq   : -0.000 ppm
/// Skew            : 0.201 ppm
/// Root delay      : 0.015881635 seconds
/// Root dispersion : 0.001088051 seconds
/// Update interval : 513.7 seconds
/// Leap status     : Normal
/// ```
///
/// Example output when not synchronized:
/// ```text
/// Reference ID    : 00000000 ()
/// Stratum         : 0
/// ```
fn parse_chrony_reference_id(output: &str) -> Option<String> {
    let line = output
        .lines()
        .find(|l| l.trim_start().starts_with("Reference ID"))?;

    let value = line.split_once(':')?.1;
    // value looks like "C035676C (ptbtime1.ptb.de)" or "00000000 ()"
    let hex_id = value.split_whitespace().next()?;

    if hex_id == "00000000" {
        return None;
    }

    Some(hex_id.to_owned())
}

pub async fn get_current_timestamp(session: &RemoteSession) -> Result<String> {
    let timestamp_result = session
        .execute_command("TERM=dumb date '+%Y-%m-%d %H:%M:%S'")
        .await
        .wrap_err("Failed to get current timestamp")?;

    ensure!(
        timestamp_result.is_success(),
        "Failed to get timestamp: {}",
        timestamp_result.stderr
    );

    Ok(timestamp_result.stdout.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chrony_reference_id_returns_id_when_synced() {
        let output = "Reference ID    : C035676C (ptbtime1.ptb.de)\n\
                      Stratum         : 2\n\
                      Ref time (UTC)  : Wed Apr 22 15:35:57 2026\n";
        assert_eq!(
            parse_chrony_reference_id(output),
            Some("C035676C".to_owned())
        );
    }

    #[test]
    fn parse_chrony_reference_id_returns_none_when_unsynced() {
        let output = "Reference ID    : 00000000 ()\n\
                      Stratum         : 0\n";
        assert_eq!(parse_chrony_reference_id(output), None);
    }

    #[test]
    fn parse_chrony_reference_id_returns_none_on_missing_line() {
        let output = "Stratum         : 2\n";
        assert_eq!(parse_chrony_reference_id(output), None);
    }
}
