use crate::{
    modem::{
        connection_state::ConnectionState, net_stats::NetStats, signal::LteSignal,
        Modem,
    },
    utils::{retry_for, retry_for_blocking, State},
};
use color_eyre::{eyre::eyre, Result};
use std::thread;
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};

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
    Ok(())
}

pub struct Telemetry {
    datadog: dogstatsd::Client,
}

impl Telemetry {
    pub fn gauge_reconnect_time(&self, modem_id: &str, secs: f64) {
        let tag = format!("mod_id:{modem_id}");
        let _ = self.datadog.gauge(
            "orb.lte.reconnect_time_seconds",
            secs.to_string(),
            [tag],
        );
    }

    pub fn on_poll_success(
        &self,
        _modem_id: &str,
        signal: &LteSignal,
        net_stats: &NetStats,
    ) {
        let _ = self.datadog.incr("orb.lte.heartbeat", NO_TAGS);
        let _ = self.datadog.gauge("orb.lte.online", "1", NO_TAGS);
        self.log_lte_snapshot(signal, net_stats);
    }

    pub fn on_poll_error(&self, modem_id: &str, state: ConnectionState) {
        let modem_tag = format!("modem_id:{modem_id}");
        let state_tag = format!(
            "conn_state:{}",
            match &state {
                ConnectionState::Connected => "connected",
                ConnectionState::Connecting => "connecting",
                ConnectionState::Registered => "registered",
                ConnectionState::Searching => "searching",
                ConnectionState::Disconnecting => "disconnecting",
                ConnectionState::Enabling => "enabling",
                ConnectionState::Enabled => "enabled",
                ConnectionState::Disabled => "disabled",
                ConnectionState::Failed => "failed",
                ConnectionState::Locked => "locked",
                ConnectionState::Unknown(v) => v.as_str(),
            }
        );

        let _ = self.datadog.incr(
            "orb.lte.poll_error",
            [modem_tag.as_str(), state_tag.as_str()],
        );
        let _ = self
            .datadog
            .gauge("orb.lte.online", "0", [modem_tag.as_str()]);
    }

    pub fn log_lte_snapshot(&self, sig: &LteSignal, ns: &NetStats) {
        if let Some(v) = sig.rsrp {
            let _ = self
                .datadog
                .gauge("orb.lte.signal.rsrp", v.to_string(), NO_TAGS);
        }
        if let Some(v) = sig.rsrq {
            let _ = self
                .datadog
                .gauge("orb.lte.signal.rsrq", v.to_string(), NO_TAGS);
        }
        if let Some(v) = sig.rssi {
            let _ = self
                .datadog
                .gauge("orb.lte.signal.rssi", v.to_string(), NO_TAGS);
        }
        if let Some(v) = sig.snr {
            let _ = self
                .datadog
                .gauge("orb.lte.signal.snr", v.to_string(), NO_TAGS);
        }

        let _ = self.datadog.gauge(
            "orb.lte.net.rx_bytes",
            ns.rx_bytes.to_string(),
            NO_TAGS,
        );
        let _ = self.datadog.gauge(
            "orb.lte.net.tx_bytes",
            ns.tx_bytes.to_string(),
            NO_TAGS,
        );
        // Location (MCC/MNC/TAC/CID) intentionally NOT sent (kept for Fleet)
    }
    pub fn incr_reconnect(&self, _modem_id: &str) {
        let _ = self.datadog.incr("orb.lte.count.reconnect", NO_TAGS);
    }
}
