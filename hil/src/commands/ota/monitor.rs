use crate::remote_cmd::RemoteSession;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use std::time::{Duration, Instant};
use tracing::warn;

const MAX_WAIT_SECONDS: u64 = 7200;
const POLL_INTERVAL: u64 = 3;
const MAX_CONSECUTIVE_FAILURES: u32 = 10;

pub async fn check_service_failed(session: &RemoteSession) -> Result<bool> {
    let result = session
        .execute_command(
            "TERM=dumb sudo systemctl is-failed worldcoin-update-agent.service",
        )
        .await
        .wrap_err("Failed to check service status")?;

    Ok(result.exit_status == 0)
}

/// Monitor update progress by polling journalctl logs
pub async fn monitor_update_progress(
    session: &RemoteSession,

    // This timestamp is coming directly from the Orb
    // And is used to get logs from journalctl --since
    start_timestamp: &str,
) -> Result<Vec<String>> {
    // This start time is used for the timeout
    let start_time = Instant::now();
    let timeout = Duration::from_secs(MAX_WAIT_SECONDS);
    let mut cursor: Option<String> = None;
    let mut all_lines = Vec::new();
    let mut consecutive_failures = 0;

    while start_time.elapsed() < timeout {
        match check_service_failed(session).await {
            Ok(true) => {
                // Service failed - fetch remaining logs to show the actual error
                if let Ok((error_lines, _)) =
                    fetch_new_log_lines(session, cursor.as_deref(), start_timestamp)
                        .await
                {
                    for line in &error_lines {
                        println!("{}", line.trim());
                    }
                    all_lines.extend(error_lines);
                }

                // Also fetch the service status for more details
                let status_result = session
                    .execute_command(
                        "TERM=dumb sudo systemctl status worldcoin-update-agent.service --no-pager -l",
                    )
                    .await;

                if let Ok(result) = status_result {
                    println!("\n=== Service Status ===");
                    println!("{}", result.stdout);
                }

                bail!("Update agent service failed - update installation failed. Check logs above for details.");
            }
            Ok(false) => {
                // Service is not failed, continue monitoring
            }
            Err(e) => {
                warn!("Error checking service status: {}", e);
            }
        }

        match fetch_new_log_lines(session, cursor.as_deref(), start_timestamp).await {
            Ok((new_lines, new_cursor)) => {
                consecutive_failures = 0;
                cursor = new_cursor;

                for line in new_lines {
                    println!("{}", line.trim());

                    // Check for reboot message - this is the success signal
                    if line.contains("waiting 10 seconds before reboot to allow propagation to backend") {
                        all_lines.push(line);
                        return Ok(all_lines);
                    }
                    all_lines.push(line);
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                warn!(
                    "Error fetching log lines (attempt {}): {}",
                    consecutive_failures, e
                );

                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    bail!("Too many consecutive failures fetching update logs");
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL)).await;
    }

    bail!(
        "Timeout waiting for update completion within {} seconds",
        MAX_WAIT_SECONDS
    );
}

async fn fetch_new_log_lines(
    session: &RemoteSession,
    cursor: Option<&str>,
    start_timestamp: &str,
) -> Result<(Vec<String>, Option<String>)> {
    let command = if let Some(cursor) = cursor {
        format!(
            "TERM=dumb sudo journalctl -u worldcoin-update-agent.service --no-pager --after-cursor='{cursor}' --show-cursor"
        )
    } else {
        format!(
            "TERM=dumb sudo journalctl -u worldcoin-update-agent.service --no-pager --since '{start_timestamp}' --show-cursor"
        )
    };

    let result = session
        .execute_command(&command)
        .await
        .wrap_err("Failed to fetch journalctl logs")?;

    if !result.is_success() {
        warn!("Failed to fetch journalctl logs: {}", result.stderr);
        return Ok((Vec::new(), cursor.map(|s| s.to_string())));
    }

    let mut lines: Vec<&str> = result.stdout.lines().collect();
    let mut new_cursor = None;

    if let Some(last_line) = lines.last()
        && let Some(cursor_value) = last_line.strip_prefix("-- cursor: ")
    {
        new_cursor = Some(cursor_value.to_string());

        // Remove cursor line from output
        lines.pop();
    }

    let new_lines: Vec<String> = lines
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect();

    Ok((new_lines, new_cursor))
}
