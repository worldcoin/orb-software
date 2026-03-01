use crate::remote_cmd::RemoteSession;
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result,
};

const TMP_OS_RELEASE_PATH: &str = "/tmp/os-release";
const ETC_OS_RELEASE_PATH: &str = "/etc/os-release";
const OS_RELEASE_HEREDOC_MARKER: &str = "__ORB_HIL_OTA_OS_RELEASE__";
const GONDOR_CALLS_FOR_OTA_PATH: &str = "/usr/local/bin/gondor-calls-for-ota";

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

/// Kick off the update-agent flow for OTA using gondor-calls-for-ota.
///
/// If the target includes `-{platform}-{release}` and the release part differs
/// from os-release, we patch the mounted os-release and restart update-agent
/// before invoking gondor.
pub async fn kickoff_update_agent_for_ota(
    session: &RemoteSession,
    target_version: &str,
) -> Result<String> {
    let parsed_target = parse_target_version(target_version)?;
    maybe_update_os_release_release_type(
        session,
        parsed_target.release_type.as_deref(),
    )
    .await?;

    let start_timestamp = get_current_timestamp(session).await?;
    run_gondor_calls_for_ota(session, &parsed_target.gondor_target).await?;

    Ok(start_timestamp)
}

async fn run_gondor_calls_for_ota(
    session: &RemoteSession,
    target_version: &str,
) -> Result<()> {
    let cleanup_result = session
        .execute_command(
            "TERM=dumb sudo sh -c 'umount /tmp/os-release >/dev/null 2>&1 || true; rm -f /tmp/os-release >/dev/null 2>&1 || true'",
        )
        .await
        .wrap_err("Failed to clean /tmp/os-release before gondor")?;

    ensure!(
        cleanup_result.is_success(),
        "Failed to clean /tmp/os-release before gondor: {}",
        cleanup_result.stderr
    );

    let escaped_target = shell_single_quote_escape(target_version.trim());
    let command =
        format!("TERM=dumb sudo {GONDOR_CALLS_FOR_OTA_PATH} '{escaped_target}'");
    let result = session
        .execute_command(&command)
        .await
        .wrap_err("Failed to execute gondor-calls-for-ota")?;

    ensure!(
        result.is_success(),
        "gondor-calls-for-ota failed: {}",
        if result.stderr.trim().is_empty() {
            result.stdout.trim()
        } else {
            result.stderr.trim()
        }
    );

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTargetVersion {
    gondor_target: String,
    release_type: Option<String>,
}

fn parse_target_version(target_version: &str) -> Result<ParsedTargetVersion> {
    let trimmed_target = target_version.trim();
    ensure!(!trimmed_target.is_empty(), "target version cannot be empty");

    let (base_target, release_type) = parse_target_suffix(trimmed_target)?;
    let gondor_target = if base_target.starts_with("to-") {
        base_target.to_owned()
    } else {
        format!("to-{base_target}")
    };

    Ok(ParsedTargetVersion {
        gondor_target,
        release_type,
    })
}

fn parse_target_suffix(target: &str) -> Result<(&str, Option<String>)> {
    let mut parts = target.rsplitn(3, '-');
    let maybe_release = parts.next();
    let maybe_platform = parts.next();
    let maybe_base = parts.next();

    let (Some(release), Some(platform), Some(base)) =
        (maybe_release, maybe_platform, maybe_base)
    else {
        return Ok((target, None));
    };

    if platform != "diamond" && platform != "pearl" {
        return Ok((target, None));
    }

    ensure!(!base.is_empty(), "invalid target format: {target}");

    Ok((base, Some(release.to_owned())))
}

async fn maybe_update_os_release_release_type(
    session: &RemoteSession,
    target_release_type: Option<&str>,
) -> Result<()> {
    let Some(target_release_type) = target_release_type else {
        return Ok(());
    };

    let os_release_path = resolve_os_release_path(session).await?;
    let os_release_content = read_remote_file(session, &os_release_path).await?;
    let current_release_type =
        parse_os_release_field(&os_release_content, "ORB_OS_RELEASE_TYPE")?;

    if current_release_type == target_release_type {
        return Ok(());
    }

    let updated_os_release = update_os_release_field(
        &os_release_content,
        "ORB_OS_RELEASE_TYPE",
        target_release_type,
    )?;
    write_remote_file(session, &os_release_path, &updated_os_release).await?;
    restart_update_agent(session).await?;

    Ok(())
}

fn shell_single_quote_escape(value: &str) -> String {
    value.replace('\'', "'\"'\"'")
}

async fn resolve_os_release_path(session: &RemoteSession) -> Result<String> {
    let result = session
        .execute_command(&format!(
            "TERM=dumb sh -c 'if [ -f {TMP_OS_RELEASE_PATH} ]; then echo {TMP_OS_RELEASE_PATH}; else echo {ETC_OS_RELEASE_PATH}; fi'"
        ))
        .await
        .wrap_err("Failed to resolve os-release path")?;

    ensure!(
        result.is_success(),
        "Failed to resolve os-release path: {}",
        result.stderr
    );

    let path = result.stdout.trim();
    ensure!(
        path == TMP_OS_RELEASE_PATH || path == ETC_OS_RELEASE_PATH,
        "Unexpected os-release path: {}",
        path
    );

    Ok(path.to_string())
}

fn parse_os_release_field(content: &str, key: &'static str) -> Result<String> {
    for line in content.lines() {
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
            continue;
        }

        let Some((field, value)) = trimmed_line.split_once('=') else {
            continue;
        };

        if field.trim() == key {
            return Ok(value.trim().trim_matches('"').to_owned());
        }
    }

    bail!("{key} field not found in os-release")
}

fn update_os_release_field(
    content: &str,
    key: &'static str,
    target_value: &str,
) -> Result<String> {
    let mut has_key = false;
    let mut updated_lines = Vec::new();

    for line in content.lines() {
        let Some((field, value)) = line.split_once('=') else {
            updated_lines.push(line.to_owned());
            continue;
        };

        if field.trim() != key {
            updated_lines.push(line.to_owned());
            continue;
        }

        let updated_key =
            if value.trim().starts_with('"') && value.trim().ends_with('"') {
                format!(r#"{key}="{target_value}""#)
            } else {
                format!("{key}={target_value}")
            };

        updated_lines.push(updated_key);
        has_key = true;
    }

    ensure!(has_key, "{key} field not found in os-release");

    let mut updated = updated_lines.join("\n");
    if content.ends_with('\n') {
        updated.push('\n');
    }

    Ok(updated)
}

async fn read_remote_file(session: &RemoteSession, path: &str) -> Result<String> {
    let result = session
        .execute_command(&format!("TERM=dumb cat {path}"))
        .await
        .wrap_err_with(|| format!("Failed to read {path}"))?;

    ensure!(
        result.is_success(),
        "Failed to read {path}: {}",
        result.stderr
    );

    Ok(result.stdout)
}

async fn write_remote_file(
    session: &RemoteSession,
    path: &str,
    content: &str,
) -> Result<()> {
    ensure!(
        !content
            .lines()
            .any(|line| line.trim() == OS_RELEASE_HEREDOC_MARKER),
        "os-release content contains an unsupported heredoc marker"
    );

    let command = format!(
        "TERM=dumb cat <<'{OS_RELEASE_HEREDOC_MARKER}' | sudo tee {path} > /dev/null\n{content}\n{OS_RELEASE_HEREDOC_MARKER}"
    );
    let result = session
        .execute_command(&command)
        .await
        .wrap_err_with(|| format!("Failed to write {path}"))?;

    ensure!(
        result.is_success(),
        "Failed to write {path}: {}",
        result.stderr
    );

    Ok(())
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
            .execute_command("TERM=dumb timedatectl status")
            .await
            .wrap_err("Failed to check time synchronization status")?;

        if result.is_success()
            && (result.stdout.contains("System clock synchronized: yes")
                || result.stdout.contains("synchronized: yes"))
        {
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
pub async fn restart_update_agent(session: &RemoteSession) -> Result<String> {
    let start_timestamp = get_current_timestamp(session).await?;

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
    fn parse_target_version_adds_to_prefix() {
        let parsed = parse_target_version("0.0.420").unwrap();
        assert_eq!(parsed.gondor_target, "to-0.0.420");
        assert_eq!(parsed.release_type, None);
    }

    #[test]
    fn parse_target_version_keeps_existing_prefix() {
        let parsed = parse_target_version("to-latest").unwrap();
        assert_eq!(parsed.gondor_target, "to-latest");
        assert_eq!(parsed.release_type, None);
    }

    #[test]
    fn parse_target_version_strips_platform_and_release_suffix() {
        let parsed = parse_target_version("to-0.0.420-diamond-dev").unwrap();
        assert_eq!(parsed.gondor_target, "to-0.0.420");
        assert_eq!(parsed.release_type, Some("dev".to_string()));
    }

    #[test]
    fn parse_target_version_accepts_staging_alias() {
        let parsed = parse_target_version("to-0.0.420-pearl-staging").unwrap();
        assert_eq!(parsed.gondor_target, "to-0.0.420");
        assert_eq!(parsed.release_type, Some("staging".to_string()));
    }

    #[test]
    fn parse_target_version_keeps_unknown_release_suffix() {
        let parsed = parse_target_version("to-0.0.420-diamond-qa").unwrap();
        assert_eq!(parsed.gondor_target, "to-0.0.420");
        assert_eq!(parsed.release_type, Some("qa".to_string()));
    }

    #[test]
    fn parse_os_release_field_reads_expected_values() {
        let content = r#"ORB_OS_PLATFORM_TYPE=diamond
ORB_OS_RELEASE_TYPE=dev
ORB_OS_VERSION=7.6.0
"#;

        let platform = parse_os_release_field(content, "ORB_OS_PLATFORM_TYPE").unwrap();
        let release = parse_os_release_field(content, "ORB_OS_RELEASE_TYPE").unwrap();

        assert_eq!(platform, "diamond");
        assert_eq!(release, "dev");
    }

    #[test]
    fn update_os_release_field_updates_release_type() {
        let content = r#"NAME="Ubuntu"
ORB_OS_PLATFORM_TYPE=diamond
ORB_OS_RELEASE_TYPE=dev
ORB_OS_VERSION=7.6.0
"#;

        let updated =
            update_os_release_field(content, "ORB_OS_RELEASE_TYPE", "prod").unwrap();

        assert!(updated.contains("ORB_OS_RELEASE_TYPE=prod"));
        assert!(!updated.contains("ORB_OS_RELEASE_TYPE=dev"));
    }

    #[test]
    fn shell_single_quote_escape_escapes_correctly() {
        let escaped = shell_single_quote_escape("abc'def");
        assert_eq!(escaped, "abc'\"'\"'def");
    }
}
