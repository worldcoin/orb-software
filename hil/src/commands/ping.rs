use clap::Parser;
use color_eyre::{eyre::eyre, eyre::WrapErr as _, Result};
use tokio::time::{timeout, Duration};
use tracing::debug;

use crate::OrbConfig;

/// Wait until the orb is reachable via mDNS ping
#[derive(Debug, Parser)]
pub struct Ping {
    #[arg(long, default_value = "120s", value_parser = humantime::parse_duration)]
    timeout: Duration,

    #[arg(long, default_value = "1s", value_parser = humantime::parse_duration)]
    interval: Duration,
}

impl Ping {
    pub async fn run(self, orb_config: &OrbConfig) -> Result<()> {
        let hostname = orb_config
            .get_hostname()
            .ok_or_else(|| eyre!("orb-id or hostname must be specified"))?;

        let poll = async {
            loop {
                let status = tokio::process::Command::new("ping")
                    .args(["-c", "1", "-W", "1", &hostname])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await
                    .wrap_err("failed to spawn ping")?;

                if status.success() {
                    return Ok::<(), color_eyre::Report>(());
                }

                debug!("ping to {hostname} failed, retrying...");
                tokio::time::sleep(self.interval).await;
            }
        };

        match timeout(self.timeout, poll).await {
            Ok(result) => {
                result?;
                println!("Orb is reachable");
                Ok(())
            }
            Err(_) => {
                println!("Orb unreachable after 2 minutes");
                Err(eyre!("timed out waiting for orb to become reachable"))
            }
        }
    }
}
