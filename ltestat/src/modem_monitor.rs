use std::time::Duration;

use crate::{lte_data::LteStat, utils};

use super::connection_state::ConnectionState;
use super::utils::run_cmd;
use chrono::{DateTime, Utc};
use color_eyre::{eyre::OptionExt, Result};
use tokio::time::{sleep, Instant};

/// Holds modem identity and connection tracking
pub struct ModemMonitor {
    pub modem_id: String,

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
    pub async fn new() -> Result<Self> {
        // Get the modem ID used by `mmcli`
        let output = run_cmd("mmcli", &["-L"]).await?;
        let modem_id = output
            .split_whitespace()
            .next()
            .and_then(|path| path.rsplit('/').next())
            .ok_or_eyre("Failed to get modem id")?
            .to_owned();

        Ok(Self {
            modem_id,
            state: ConnectionState::Unknown,
            last_state: None,
            disconnected_since: None,
            last_disconnected_at: None,
            last_connected_at: None,
            disconnected_count: 0,
            last_snapshot: None,
            last_downtime_secs: None,
        })
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
                // TODO: Handle logging when not connected;
                self.dump_info();
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

    pub fn dump_info(&self) {
        println!("=== Modem Monitor Status ===");
        println!("Modem ID: {}", self.modem_id);
        println!("Current State: {:?}", self.state);
        println!("Last State: {:?}", self.last_state);

        println!("Disconnected Count: {}", self.disconnected_count);

        if let Some(dt) = &self.last_disconnected_at {
            println!("Last Disconnected At: {}", dt.to_rfc3339());
        } else {
            println!("Last Disconnected At: never");
        }

        if let Some(dt) = &self.last_connected_at {
            println!("Last Connected At: {}", dt.to_rfc3339());
        } else {
            println!("Last Connected At: never");
        }

        if let Some(secs) = self.last_downtime_secs {
            println!("Last Downtime: {:.1} seconds", secs);
        } else if self.disconnected_since.is_some() {
            println!("Currently Disconnected — downtime still in progress");
        } else {
            println!("Last Downtime: n/a");
        }

        println!("============================");
    }
}
