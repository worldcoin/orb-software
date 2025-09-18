use crate::{
    telemetry::{modem_status::ModemStatus, net_stats::NetStats},
    utils::{retry_for, State},
};
use color_eyre::{eyre::eyre, Result};
use std::{sync::Arc, time::Duration};
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info};

const NO_TAGS: &[&str] = &[];

pub fn start(
    modem: State<ModemStatus>,
    report_interval: Duration,
) -> JoinHandle<Result<()>> {
    info!("starting dd reporter");
    task::spawn(async move {
        let dd_client =
            retry_for(Duration::MAX, Duration::from_secs(20), make_dd_client).await?;

        info!("successfully created dogstatd::Client");

        let dd_client = Arc::new(dd_client);
        let mut prev_net_stats = modem
            .read(|m| m.net_stats.clone())
            .map_err(|e| eyre!("dd_repoter::start, modem.read: {e}"))?;

        loop {
            if let Err(e) =
                report(modem.clone(), dd_client.clone(), &mut prev_net_stats).await
            {
                error!("failed to report to backend status: {e}");
            }

            time::sleep(report_interval).await;
        }
    })
}

async fn make_dd_client() -> Result<dogstatsd::Client> {
    task::spawn_blocking(|| {
        let opts = dogstatsd::Options::default();
        let client = dogstatsd::Client::new(opts)?;
        Ok(client)
    })
    .await
    .map_err(|e| eyre!("failed to join make_dd_client thread: {e}"))?
}

async fn report(
    modem: State<ModemStatus>,
    dd_client: Arc<dogstatsd::Client>,
    prev_net_stats: &mut NetStats,
) -> Result<()> {
    let (state, rat, operator, gauges, new_net_stats) = modem
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

    task::spawn_blocking(move || {
        if state.is_online() {
            let heartbeat_tags: Vec<String> = [
                rat.map(|r| format!("rat:{r}")),
                operator.map(|o| format!("operator:{o}")),
            ]
            .into_iter()
            .flatten()
            .collect();

            let _ = dd_client.count("orb.lte.heartbeat", 1, heartbeat_tags);
        }

        gauges
            .into_iter()
            .filter_map(|(name, opt)| opt.map(|v| (name, v)))
            .for_each(|(name, v)| {
                let _ = dd_client.gauge(name, v.to_string(), NO_TAGS);
            });

        let _ = dd_client.incr_by_value(
            "orb.lte.net.rx_bytes_delta",
            net_stats_delta.rx_bytes as i64,
            NO_TAGS,
        );

        let _ = dd_client.incr_by_value(
            "orb.lte.net.tx_bytes_delta",
            net_stats_delta.tx_bytes as i64,
            NO_TAGS,
        );

        Ok(())
    })
    .await
    .map_err(|e| eyre!("failed to join dd_reporter::report thread: {e}"))?
}
