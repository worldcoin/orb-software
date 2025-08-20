use std::time::Duration;

use crate::lte_data::{MmcliLocationRoot, MmcliSignalRoot, NetStats};
use crate::modem::Modem;
use crate::utils::State;
use crate::{lte_data::LteStat, utils};

use super::connection_state::ConnectionState;
use super::utils::{retrieve_value, run_cmd};
use chrono::{DateTime, Utc};
use color_eyre::eyre::{bail, eyre};
use color_eyre::Result;
use tokio::task::{self, JoinHandle};
use tokio::time::{self, sleep, Instant};

type Rat = String;
type Operator = String;

pub fn start(modem: State<Modem>, poll_interval: Duration) -> JoinHandle<()> {
    task::spawn(async move {
        loop {
            let modem_id = modem.read(|m| m.id.clone()).unwrap();
            let (connection_state, rat, operator) =
                get_connection_status(&modem_id).await.unwrap();
            let lte_stats = get_lte_stats(&modem_id).await.unwrap();

            // TODO: deal with modem id changing
            // TODO: deal with signal when disconnected
            // TODO: add disconnected count
            modem
                .write(|m| {
                    m.last_state = Some(m.state);
                    m.state = connection_state;
                    m.rat = Some(rat);
                    m.operator = Some(operator);
                    m.last_snapshot = Some(lte_stats);
                })
                .unwrap();
            time::sleep(poll_interval).await;
        }
    })
}

async fn get_connection_status(
    modem_id: &str,
) -> Result<(ConnectionState, Rat, Operator)> {
    let state = ConnectionState::get_connection_state(modem_id).await?;
    println!("Waiting for modem {} to reconnect...", modem_id);

    if !state.is_online() {
        bail!("Modem is {:?}", state);
    }

    // If we are online, grab the carier and rat
    let output = run_cmd("mmcli", &["-m", modem_id, "--output-keyvalue"]).await?;

    let operator: String = retrieve_value(&output, "modem.3gpp.operator-name")?;

    // TODO: must be more precise
    let rat: String =
        retrieve_value(&output, "modem.generic.access-technologies.value[1] ")?;

    println!("Modem {} reconnected", modem_id);

    // Needed for mmcli to enable signal monitoring. 10 is update time
    // Can be called multiple times
    utils::run_cmd("mmcli", &["-m", modem_id, "--signal-setup", "10"]).await?;

    return Ok((state, rat, operator));
}

async fn get_lte_stats(modem_id: &str) -> Result<LteStat> {
    let signal_output =
        run_cmd("mmcli", &["-m", modem_id, "--signal-get", "--output-json"]).await?;

    // TODO: get signal info based on current tech
    let signal: MmcliSignalRoot = serde_json::from_str(&signal_output)?;
    let signal = signal.modem.signal.lte;

    let location_output = run_cmd(
        "mmcli",
        &["-m", modem_id, "--location-get", "--output-json"],
    )
    .await?;

    let location: MmcliLocationRoot = serde_json::from_str(&location_output)?;

    let location = location.modem.location.gpp;

    let net_stats = NetStats::new().await?;

    Ok(LteStat {
        // timestamp,
        signal,
        location,
        net_stats: Some(net_stats),
    })
}
