use crate::{modem, statsd::StatsdClient};
use color_eyre::Result;
use flume::Receiver;
use speare::mini;
use std::{collections::HashMap, sync::Arc};
use tracing::{info, warn};

pub struct Args {
    pub statsd: Arc<dyn StatsdClient>,
}

pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting datadog reporter");

    async {
        let modem_snapshot_rx: Receiver<modem::Snapshot> =
            ctx.subscribe("modem-snapshot")?;

        let netstats_rx: Receiver<Vec<oes::NetStats>> = ctx.subscribe("netstats")?;
        let mut netstats_map: HashMap<String, oes::NetStats> = HashMap::new();

        loop {
            tokio::select! {
                Ok(snapshot) = modem_snapshot_rx.recv_async() => {
                    report_modem(ctx.statsd.as_ref(), snapshot).await?;
                }

                Ok(all_netstats) = netstats_rx.recv_async() => {
                    for new_netstats in all_netstats {
                        let old_netstats = netstats_map.remove(&new_netstats.iface)
                            .unwrap_or_else(|| new_netstats.clone());

                        report_netstats(ctx.statsd.as_ref(), &old_netstats, &new_netstats).await?;

                        netstats_map.insert(new_netstats.iface.clone(), new_netstats);
                    }

                }
            }
        }

        #[allow(unreachable_code)]
        Ok(())
    }
    .await
    .inspect_err(|e| warn!("failure reporting to datadog {e:?}"))
}

async fn report_modem(statsd: &dyn StatsdClient, m: modem::Snapshot) -> Result<()> {
    let sig = m.signal;

    let gauges = vec![
        ("orb.lte.signal.rsrp", sig.rsrp),
        ("orb.lte.signal.rsrq", sig.rsrq),
        ("orb.lte.signal.rssi", sig.rssi),
        ("orb.lte.signal.snr", sig.snr),
    ];

    if m.state.is_online() {
        let heartbeat_tags: Vec<String> = [
            m.rat.map(|r| format!("rat:{r}")),
            m.operator.map(|o| format!("operator:{o}")),
        ]
        .into_iter()
        .flatten()
        .collect();

        statsd.count("orb.lte.heartbeat", 1, heartbeat_tags).await?;
    }

    for (name, v) in gauges
        .into_iter()
        .filter_map(|(name, opt)| opt.map(|v| (name, v)))
    {
        statsd.gauge(name, &v.to_string(), Vec::new()).await?;
    }

    Ok(())
}

async fn report_netstats(
    statsd: &dyn StatsdClient,
    old_netstats: &oes::NetStats,
    new_netstats: &oes::NetStats,
) -> Result<()> {
    let rx_bytes = new_netstats.rx_bytes - old_netstats.rx_bytes;
    let tx_bytes = new_netstats.tx_bytes - old_netstats.tx_bytes;

    if rx_bytes == 0 && tx_bytes == 0 {
        return Ok(());
    }

    statsd
        .incr_by_value(
            &format!("orb.{}.net.rx_bytes_delta", new_netstats.iface),
            rx_bytes as i64,
            Vec::new(),
        )
        .await?;

    statsd
        .incr_by_value(
            &format!("orb.{}.net.tx_bytes_delta", new_netstats.iface),
            tx_bytes as i64,
            Vec::new(),
        )
        .await?;

    Ok(())
}
