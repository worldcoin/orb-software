use crate::ssh_wrapper::SshWrapper;
use color_eyre::{
    eyre::{ensure, eyre, WrapErr},
    Result,
};
use tracing::{info, instrument, warn};

use super::Ota;

impl Ota {
    #[instrument(skip_all)]
    pub(super) async fn run_update_verifier(&self, session: &SshWrapper) -> Result<()> {
        info!("Running orb-update-verifier");

        let result = session
            .execute_command("TERM=dumb sudo orb-update-verifier")
            .await
            .wrap_err("Failed to run orb-update-verifier")?;

        ensure!(
            result.is_success(),
            "orb-update-verifier failed: {}",
            result.stderr
        );

        info!("orb-update-verifier succeeded: {}", result.stdout);
        Ok(())
    }

    #[instrument(skip_all)]
    pub(super) async fn get_capsule_update_status(
        &self,
        session: &SshWrapper,
    ) -> Result<()> {
        info!("Getting capsule update status");

        let result = session
            .execute_command("TERM=dumb sudo nvbootctrl dump-slots-info")
            .await
            .wrap_err("Failed to get capsule update status")?;

        // Note: nvbootctrl returns exit code 1 with "Error: can not open /dev/mem" but still outputs valid info
        // So we don't check is_success() here, just parse the output

        let capsule_status = result
            .stdout
            .lines()
            .find(|line| line.starts_with("Capsule update status:"))
            .and_then(|line| line.split(':').nth(1).map(|s| s.trim().to_string()))
            .ok_or_else(|| {
                eyre!("Could not find 'Capsule update status' in nvbootctrl output")
            })?;

        println!("CAPSULE_UPDATE_STATUS={}", capsule_status);

        info!("Capsule update status: {}", capsule_status);
        Ok(())
    }

    #[instrument(skip_all)]
    pub(super) async fn run_check_my_orb(&self, session: &SshWrapper) -> Result<()> {
        info!("Running check-my-orb");

        let result = session
            .execute_command("TERM=dumb check-my-orb")
            .await
            .wrap_err("Failed to run check-my-orb")?;

        if !result.is_success() {
            warn!("check-my-orb failed with exit code: {}", result.stderr);
            println!("CHECK_MY_ORB_STATUS=FAILED");
        } else {
            println!("CHECK_MY_ORB_STATUS=SUCCESS");
            info!("check-my-orb completed successfully");
        }

        println!("CHECK_MY_ORB_OUTPUT_START");
        println!("{}", result.stdout);
        println!("CHECK_MY_ORB_OUTPUT_END");

        Ok(())
    }

    #[instrument(skip_all)]
    pub(super) async fn get_boot_time(&self, session: &SshWrapper) -> Result<()> {
        info!("Getting last boot time");

        let result = session
            .execute_command("TERM=dumb systemd-analyze time")
            .await
            .wrap_err("Failed to run systemd-analyze")?;

        ensure!(
            result.is_success(),
            "systemd-analyze failed: {}",
            result.stderr
        );

        println!("BOOT_TIME");
        println!("{}", result.stdout);
        Ok(())
    }
}
