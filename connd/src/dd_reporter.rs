use crate::{
    modem::Modem,
    utils::{retry_for, State},
};
use color_eyre::{eyre::eyre, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::error;

const NO_TAGS: &[&str] = &[];

pub fn start(modem: State<Modem>, report_interval: Duration) -> JoinHandle<Result<()>> {
    task::spawn(async move {
        let dd_client =
            retry_for(Duration::MAX, Duration::from_secs(20), make_dd_client).await?;

        let dd_client = Arc::new(dd_client);

        loop {
            if let Err(e) = report(modem.clone(), dd_client.clone()).await {
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

async fn report(modem: State<Modem>, dd_client: Arc<dogstatsd::Client>) -> Result<()> {
    task::spawn_blocking(move || {
        let (state, rat, operator, gauges) = modem
            .read(|m| {
                let sig = m.signal.as_ref();
                let ns = m.net_stats.as_ref();

                let gauges = vec![
                    ("orb.lte.signal.rsrp", sig.and_then(|s| s.rsrp)),
                    ("orb.lte.signal.rsrq", sig.and_then(|s| s.rsrq)),
                    ("orb.lte.signal.rssi", sig.and_then(|s| s.rssi)),
                    ("orb.lte.signal.snr", sig.and_then(|s| s.snr)),
                    ("orb.lte.net.rx_bytes", ns.map(|n| n.rx_bytes as f64)),
                    ("orb.lte.net.tx_bytes", ns.map(|n| n.tx_bytes as f64)),
                ];

                (m.state.clone(), m.rat.clone(), m.operator.clone(), gauges)
            })
            .map_err(|e| {
                eyre!("failed to read ConnectionState from State<Modem>: {e:?}")
            })?;

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

        Ok(())
    })
    .await
    .map_err(|e| eyre!("failed to join dd_reporter::report thread: {e}"))?
}
