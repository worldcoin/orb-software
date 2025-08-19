use std::time::Duration;

use crate::{lte_data::LteStat, utils};

use super::connection_state::ConnectionState;
use super::utils::{retrieve_value, run_cmd};
use chrono::{DateTime, Utc};
use color_eyre::Result;
use tokio::time::{sleep, Instant};

/// Holds modem identity and connection tracking
pub struct ModemMonitor {
    pub modem_id: String,
    pub iccid: String,
    pub imei: String,
    pub rat: Option<String>,
    pub operator: Option<String>,

    pub state: ConnectionState,
    pub last_state: Option<ConnectionState>,
    pub disconnected_since: Option<Instant>,
    pub last_disconnected_at: Option<DateTime<Utc>>,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub disconnected_count: u64,
    pub last_snapshot: Option<LteStat>,
    pub last_downtime_secs: Option<f64>,
}

impl ModemMonitor {
    pub async fn new(max_attempts: u8, mut min_delay: Duration) -> Result<Self> {
        // Get the modem ID used by `mmcli`

        // Try to find the modem 3 times, increasing the delay between tries
        for attempt in 1..=max_attempts {
            match run_cmd("mmcli", &["-L"]).await {
                Ok(output) => {
                    if let Some(modem_id) = output
                        .split_whitespace()
                        .next()
                        .and_then(|path| path.rsplit('/').next())
                        .map(|s| s.to_owned())
                    {
                        // If we manage to get modem, grab the iccid and imei next
                        let output =
                            run_cmd("mmcli", &["-m", &modem_id, "--output-keyvalue"])
                                .await?;

                        let imei = retrieve_value(
                            &output,
                            "modem.generic.equipment-identifier",
                        )?;

                        let sim_output =
                            run_cmd("mmcli", &["-i", "0", "--output-keyvalue"]).await?;

                        let iccid =
                            retrieve_value(&sim_output, "sim.properties.iccid")?;

                        return Ok(Self {
                            modem_id,
                            state: ConnectionState::Unknown,
                            last_state: None,
                            disconnected_since: None,
                            last_disconnected_at: None,
                            last_connected_at: None,
                            disconnected_count: 0,
                            last_snapshot: None,
                            last_downtime_secs: None,
                            iccid,
                            imei,
                            rat: None,
                            operator: None,
                        });
                    } else {
                        eprintln!(
                            "mmcli -L returned no devices (attempt {attempt}/3)."
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Failed to list modems (attempt {attempt}/3): {e}");
                }
            }
            if attempt < max_attempts {
                sleep(min_delay).await;
                min_delay = (min_delay * 2).min(Duration::from_secs(30));
            }
        }
        Err(color_eyre::eyre::eyre!(
            "No modem detected after 3 attempts"
        ))
    }

    pub async fn wait_for_connection(&mut self) -> Result<()> {
        loop {
            let now = Instant::now();
            let utc_now = Utc::now();

            let state =
                match ConnectionState::get_connection_state(&self.modem_id).await {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error getting the connection state: {e}");
                        ConnectionState::Unknown
                    }
                };

            self.update_state(now, utc_now, state);

            println!("Waiting for modem {} to reconnect...", self.modem_id);

            if self.state.is_online() {
                // If we are online, grab the carier and rat
                let output =
                    run_cmd("mmcli", &["-m", &self.modem_id, "--output-keyvalue"])
                        .await?;

                let operator: Option<String> =
                    retrieve_value(&output, "modem.3gpp.operator-name").ok();

                // TODO: must be more precise
                let rat: Option<String> = retrieve_value(
                    &output,
                    "modem.generic.access-technologies.value[1] ",
                )
                .ok();

                self.rat = rat;
                self.operator = operator;

                println!(
                    "Modem {} reconnected at {}",
                    self.modem_id,
                    utc_now.to_rfc3339()
                );
                utils::run_cmd(
                    "mmcli",
                    &["-m", &self.modem_id, "--signal-setup", "10"],
                )
                .await?;

                break;
            } else {
                sleep(Duration::from_secs(10)).await;
            }
        }
        Ok(())
    }

    pub fn update_state(
        &mut self,
        now_inst: Instant,
        now_utc: DateTime<Utc>,
        current: ConnectionState,
    ) {
        let was_connected = self.last_state.as_ref().is_some_and(|s| s.is_online());
        let is_connected = current.is_online();

        if was_connected && !is_connected {
            // connected -> not connected
            self.disconnected_since = Some(now_inst);
            self.last_disconnected_at = Some(now_utc);
            self.disconnected_count += 1;
            self.last_downtime_secs = None;

            // not connected -> connected
        } else if !was_connected && is_connected {
            if let Some(start) = self.disconnected_since.take() {
                self.last_downtime_secs =
                    Some(now_inst.duration_since(start).as_secs_f64());
            }
            self.last_connected_at = Some(now_utc);
        }

        self.last_state = Some(current);
        self.state = current;
    }

    pub async fn poll_lte(&mut self) -> Result<&LteStat> {
        let snap = LteStat::poll_for(&self.modem_id).await?;
        self.last_snapshot = Some(snap);
        Ok(self.last_snapshot.as_ref().unwrap())
    }
}
