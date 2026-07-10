use crate::modem;
use color_eyre::Result;
use crabwire::inject;
use flume::Receiver;
use orb_dogd::{DogstatsdClient, MetricEmitter, NO_TAGS};
use speare::mini;
use std::collections::HashMap;
use tracing::{info, warn};

#[inject(statsd: &DogstatsdClient)]
pub async fn report(ctx: mini::Ctx<()>) -> Result<()> {
    info!("starting datadog reporter");

    async {
        let modem_snapshot_rx: Receiver<modem::Snapshot> =
            ctx.subscribe("modem-snapshot")?;

        let netstats_rx: Receiver<Vec<oes::NetStats>> = ctx.subscribe("netstats")?;
        let mut netstats_map: HashMap<String, oes::NetStats> = HashMap::new();

        loop {
            tokio::select! {
                Ok(snapshot) = modem_snapshot_rx.recv_async() => {
                    report_modem(statsd, snapshot).await?;
                }

                Ok(all_netstats) = netstats_rx.recv_async() => {
                    for new_netstats in all_netstats {
                        let old_netstats = netstats_map.remove(&new_netstats.iface)
                            .unwrap_or_else(|| new_netstats.clone());

                        report_netstats(statsd, &old_netstats, &new_netstats).await?;

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

async fn report_modem(statsd: &impl MetricEmitter, m: modem::Snapshot) -> Result<()> {
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

        let _ = statsd.count("orb.lte.heartbeat", 1, heartbeat_tags);
    }

    for (name, v) in gauges
        .into_iter()
        .filter_map(|(name, opt)| opt.map(|v| (name, v)))
    {
        let _ = statsd.gauge(name, v, NO_TAGS);
    }

    Ok(())
}

async fn report_netstats(
    statsd: &impl MetricEmitter,
    old_netstats: &oes::NetStats,
    new_netstats: &oes::NetStats,
) -> Result<()> {
    let rx_bytes = new_netstats.rx_bytes - old_netstats.rx_bytes;
    let tx_bytes = new_netstats.tx_bytes - old_netstats.tx_bytes;

    if rx_bytes == 0 && tx_bytes == 0 {
        return Ok(());
    }

    let _ = statsd.count(
        format!("orb.{}.net.rx_bytes_delta", new_netstats.iface),
        rx_bytes as i64,
        NO_TAGS,
    );

    let _ = statsd.count(
        format!("orb.{}.net.tx_bytes_delta", new_netstats.iface),
        tx_bytes as i64,
        NO_TAGS,
    );

    Ok(())
}
