use crate::{
    statsd::StatsdClient,
    reporters::{modem_status::ModemStatus, net_stats::NetStats},
    utils::State,
};
use color_eyre::{eyre::eyre, Result};
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info};

const NO_TAGS: &[&str] = &[];

pub fn spawn(
    modem_status: State<ModemStatus>,
    statsd_client: impl StatsdClient,
    report_interval: Duration,
) -> JoinHandle<Result<()>> {
    info!("starting dd reporter");
    task::spawn(async move {
        info!("successfully created dogstatd::Client");

        let mut prev_net_stats = modem_status
            .read(|m| m.net_stats.clone())
            .map_err(|e| eyre!("dd_repoter::start, modem.read: {e}"))?;

        loop {
            if let Err(e) =
                report(modem_status.clone(), &statsd_client, &mut prev_net_stats).await
            {
                error!("failed to report to backend status: {e}");
            }

            time::sleep(report_interval).await;
        }
    })
}

async fn report(
    modem_status: State<ModemStatus>,
    statsd_client: &impl StatsdClient,
    prev_net_stats: &mut NetStats,
) -> Result<()> {
    let (state, rat, operator, gauges, new_net_stats) = modem_status
        .read(|m| {
            let sig = &m.signal;

            let gauges = vec![
                ("orb.lte.signal.rsrp", sig.rsrp),
                ("orb.lte.signal.rsrq", sig.rsrq),
                ("orb.lte.signal.rssi", sig.rssi),
                ("orb.lte.signal.snr", sig.snr),
            ];

            (
                m.state.clone(),
                m.rat.clone(),
                m.operator.clone(),
                gauges,
                m.net_stats.clone(),
            )
        })
        .map_err(|e| {
            eyre!("failed to read ConnectionState from State<Modem>: {e:?}")
        })?;

    let net_stats_delta = NetStats {
        rx_bytes: new_net_stats.rx_bytes - prev_net_stats.rx_bytes,
        tx_bytes: new_net_stats.tx_bytes - prev_net_stats.tx_bytes,
    };

    *prev_net_stats = new_net_stats;

    if state.is_online() {
        let heartbeat_tags: Vec<String> = [
            rat.map(|r| format!("rat:{r}")),
            operator.map(|o| format!("operator:{o}")),
        ]
        .into_iter()
        .flatten()
        .collect();

        statsd_client
            .count("orb.lte.heartbeat", 1, heartbeat_tags.as_ref())
            .await?;
    }

    for (name, v) in gauges
        .into_iter()
        .filter_map(|(name, opt)| opt.map(|v| (name, v)))
    {
        statsd_client.gauge(name, &v.to_string(), NO_TAGS).await?;
    }

    statsd_client
        .incr_by_value(
            "orb.lte.net.rx_bytes_delta",
            net_stats_delta.rx_bytes as i64,
            NO_TAGS,
        )
        .await?;

    statsd_client
        .incr_by_value(
            "orb.lte.net.tx_bytes_delta",
            net_stats_delta.tx_bytes as i64,
            NO_TAGS,
        )
        .await?;

    Ok(())
}
