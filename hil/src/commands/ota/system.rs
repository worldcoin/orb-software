use crate::ssh_wrapper::SshWrapper;
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result,
};
use serde_json::Value;
use tracing::{info, instrument};

use super::Ota;

impl Ota {
    #[instrument(skip_all)]
    pub(super) async fn reboot_orb(&self, session: &SshWrapper) -> Result<()> {
        info!("Rebooting Orb");

        session
            .execute_command("TERM=dumb orb-mcu-util reboot orb")
            .await
            .wrap_err("Failed to execute orb-mcu-util reboot orb")?;

        info!("orb-mcu-util reboot orb succeeded, now shutting down");

        session
            .execute_command("TERM=dumb sudo shutdown now")
            .await
            .wrap_err("Failed to execute shutdown now")?;

        info!("Shutdown command sent successfully");
        Ok(())
    }

    pub(super) async fn wipe_overlays(&self, session: &SshWrapper) -> Result<()> {
        let result = session
            .execute_command("bash -c 'source ~/.bash_profile 2>/dev/null || true; source ~/.bashrc 2>/dev/null || true; wipe_overlays'")
            .await
            .wrap_err("Failed to execute wipe_overlays function")?;

        ensure!(
            result.is_success(),
            "wipe_overlays function failed: {}",
            result.stderr
        );

        info!("Overlays wiped successfully");
        Ok(())
    }

    #[instrument(skip_all)]
    pub(super) async fn get_current_slot(
        &self,
        session: &SshWrapper,
    ) -> Result<String> {
        info!("Determining current slot");
        let result = session
            .execute_command("TERM=dumb orb-slot-ctrl -c")
            .await
            .wrap_err("Failed to execute orb-slot-ctrl -c")?;

        ensure!(
            result.is_success(),
            "orb-slot-ctrl -c failed: {}",
            result.stderr
        );

        let slot_letter = if result.stdout.contains('a') {
            'a'
        } else if result.stdout.contains('b') {
            'b'
        } else {
            bail!("Could not parse current slot from: {}", result.stdout);
        };

        let slot_name = format!("slot_{slot_letter}");
        info!("Current slot: {}", slot_name);
        Ok(slot_name)
    }

    #[instrument(skip_all)]
    pub(super) async fn update_versions_json(
        &self,
        session: &SshWrapper,
        current_slot: &str,
    ) -> Result<()> {
        info!(
            "Updating /usr/persistent/versions.json for slot {}",
            current_slot
        );

        let result = session
            .execute_command("TERM=dumb cat /usr/persistent/versions.json")
            .await
            .wrap_err("Failed to read /usr/persistent/versions.json")?;

        ensure!(
            result.is_success(),
            "Failed to read versions.json: {}",
            result.stderr
        );

        // Parse JSON in blocking task to avoid blocking async runtime
        let stdout = result.stdout.clone();
        let mut versions_data: Value =
            tokio::task::spawn_blocking(move || serde_json::from_str(&stdout))
                .await
                .wrap_err("JSON parsing task panicked")?
                .wrap_err("Failed to parse versions.json")?;

        let version_with_prefix = format!("to-{}", self.target_version);
        let releases = versions_data.get_mut("releases").ok_or_else(|| {
            color_eyre::eyre::eyre!("releases field not found in versions.json")
        })?;

        let releases_obj = releases.as_object_mut().ok_or_else(|| {
            color_eyre::eyre::eyre!("releases field is not an object in versions.json")
        })?;

        releases_obj.insert(
            current_slot.to_string(),
            Value::String(version_with_prefix.clone()),
        );

        info!(
            "Updated {} to version: {}",
            current_slot, version_with_prefix
        );

        // Serialize JSON in blocking task to avoid blocking async runtime
        let updated_json_str = tokio::task::spawn_blocking(move || {
            serde_json::to_string_pretty(&versions_data)
        })
        .await
        .wrap_err("JSON serialization task panicked")?
        .wrap_err("Failed to serialize updated versions.json")?;

        let result = session
            .execute_command(&format!(
                "echo '{updated_json_str}' | sudo tee /usr/persistent/versions.json > /dev/null"
            ))
            .await
            .wrap_err("Failed to write updated versions.json")?;

        ensure!(
            result.is_success(),
            "Failed to write versions.json: {}",
            result.stderr
        );

        info!("versions.json updated successfully");
        Ok(())
    }

    #[instrument(skip_all)]
    pub(super) async fn restart_update_agent(
        &self,
        session: &SshWrapper,
    ) -> Result<String> {
        info!("Restarting worldcoin-update-agent.service");

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
        info!("Captured start timestamp: {}", start_timestamp);

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

        info!("worldcoin-update-agent.service restarted successfully");
        Ok(start_timestamp)
    }
}
