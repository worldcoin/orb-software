use super::utils::run_cmd;
use color_eyre::{eyre::eyre, Result};

#[derive(Debug, Copy, Clone)]
pub enum ConnectionState {
    Connected,
    Connecting,
    Registered,
    Searching,
    Disconnecting,
    Enabling,
    Enabled,
    Disabled,
    Failed,
    Locked,
    Unknown,
}

impl ConnectionState {
    pub fn is_online(&self) -> bool {
        matches!(self, ConnectionState::Connected)
    }

    pub async fn get_connection_state(modem_id: &str) -> Result<Self> {
        let output = run_cmd("mmcli", &["-m", modem_id, "-K"]).await?;

        for line in output.lines() {
            if let Some(connection_line) = line.strip_prefix("modem.generic.state") {
                let data = connection_line
                    .split(':')
                    .nth(1)
                    .ok_or_else(|| eyre!("Invalid modem.generic.state line format"))?
                    .trim()
                    .trim_matches('\'')
                    .to_lowercase();

                println!("DATA {data}");
                let state = match data.as_str() {
                    "connected" => ConnectionState::Connected,
                    "connecting" => ConnectionState::Connecting,
                    "registered" => ConnectionState::Registered,
                    "searching" => ConnectionState::Searching,
                    "disconnecting" => ConnectionState::Disconnecting,
                    "enabling" => ConnectionState::Enabling,
                    "enabled" => ConnectionState::Enabled,
                    "disabled" => ConnectionState::Disabled,
                    "failed" => ConnectionState::Failed,
                    "locked" => ConnectionState::Locked,
                    _ => ConnectionState::Unknown,
                };

                return Ok(state);
            }
        }
        Err(eyre!("modem.generic.state not found in mmcli output"))
    }
}
