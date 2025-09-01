use crate::modem_manager;
use crate::telemetry::connection_state::ConnectionState;
use crate::telemetry::net_stats::NetStats;
use crate::telemetry::Modem;
use crate::utils::State;
use color_eyre::eyre::eyre;
use color_eyre::Result;
use std::time::Duration;
use tokio::task::{self, JoinHandle};
use tokio::time::{self};
use tracing::{error, info};

type Rat = String;
type Operator = String;

pub fn start(modem: State<Modem>, poll_interval: Duration) -> JoinHandle<Result<()>> {
    info!("starting modem monitor");
    task::spawn(async move {
        loop {
            if let Err(e) = update_modem(&modem).await {
                error!("failed to update modem: {e}");
            }

            time::sleep(poll_interval).await;
        }
    })
}

async fn update_modem(modem: &State<Modem>) -> Result<()> {
    let (current_modem_id, current_connection_state, mut disconnected_count) = modem
        .read(|m| (m.id.clone(), m.state.clone(), m.disconnected_count))
        .map_err(|e| {
            eyre!("failed to read ConnectionState from State<Modem>: {e:?}")
        })?;

    let new_modem_id = modem_manager::get_modem_id().await?;

    if new_modem_id != current_modem_id {
        modem_manager::start_signal_refresh(&new_modem_id).await?;
    }

    let (new_connection_state, operator, rat) =
        get_connection_status(&new_modem_id).await?;

    if current_connection_state.is_online() && !new_connection_state.is_online() {
        disconnected_count += 1
    };

    let signal = modem_manager::get_signal(&new_modem_id)
        .await
        .inspect_err(|e| error!("modem_manager::get_signal: err {e}"))
        .ok()
        .and_then(|s| s.modem.signal.lte);

    let location = modem_manager::get_location(&new_modem_id)
        .await
        .inspect_err(|e| error!("modem_manager::get_location: err {e}"))
        .ok()
        .and_then(|l| l.modem.location.gpp);

    let net_stats = NetStats::from_wwan0()
        .await
        .inspect_err(|e| error!("NetStats::from_wwan0: err {e}"));

    modem
        .write(|m| {
            m.id = new_modem_id;
            m.prev_state = Some(m.state.clone());
            m.state = new_connection_state;
            m.rat = rat;
            m.operator = operator;
            m.disconnected_count = disconnected_count;
            m.signal = signal;
            m.location = location;

            if let Ok(stats) = net_stats {
                m.net_stats = stats;
            }
        })
        .map_err(|e| eyre!("failed to write to State<Modem>: {e:?}"))?;

    Ok(())
}

async fn get_connection_status(
    modem_id: &str,
) -> Result<(ConnectionState, Option<Operator>, Option<Rat>)> {
    let state = modem_manager::get_connection_state(modem_id).await?;
    if !state.is_online() {
        return Ok((state, None, None));
    }

    let (operator, rat) = match modem_manager::get_operator_and_rat(modem_id).await {
        Err(e) => {
            error!("could not get operator and rat: {e}");
            (None, None)
        }

        Ok((operator, rat)) => (Some(operator), Some(rat)),
    };

    Ok((state, operator, rat))
}
