use crate::{
    modem::Modem,
    utils::{retry_for_blocking, State},
};
use color_eyre::{eyre::eyre, Result};
use std::thread;
use std::time::Duration;
use tokio::task::{self, JoinHandle};

const NO_TAGS: &[&str] = &[];

pub fn start(modem: State<Modem>, report_interval: Duration) -> JoinHandle<Result<()>> {
    task::spawn_blocking(move || {
        let dd_client =
            retry_for_blocking(Duration::MAX, Duration::from_secs(20), make_dd_client)?;

        loop {
            if let Err(e) = report(&modem, &dd_client) {
                println!("failed to repot to backend status: {e}");
            }

            thread::sleep(report_interval);
        }
    })
}

fn make_dd_client() -> Result<dogstatsd::Client> {
    let opts = dogstatsd::Options::default();
    let client = dogstatsd::Client::new(opts)?;
    Ok(client)
}

fn report(modem: &State<Modem>, dd_client: &dogstatsd::Client) -> Result<()> {
    let (state, rat, operator, gauges) = modem
        .read(|m| {
            let sig = m.signal.as_ref();
            let ns = m.net_stats.as_ref();

            let gauges = vec![
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
        .filter_map(|value| value)
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
}
