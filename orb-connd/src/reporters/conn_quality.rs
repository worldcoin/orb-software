//! Measures connection quality and reports it through OES.

use chrono::Utc;
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use crabwire::inject;
use nom::AsBytes;
use oes::{ConnectionQualityReport, Quality, Traffic};
use orb_info::OrbId;
use orb_speed_test::{
    assess_connectivity_quality, run_pcp_speed_test, run_speed_test,
    ConnectivityQuality, PcpSpeedTestResults, SpeedTestResults,
};
use serde::{Deserialize, Serialize};
use speare::mini;
use std::{collections::VecDeque, time::Duration};
use tokio::{
    select,
    time::{self, Instant, MissedTickBehavior},
};
use tracing::warn;
use zenorb::{
    zenoh::{handlers::FifoChannelHandler, pubsub::Subscriber, sample::Sample},
    Zenorb,
};

const DEFAULT_INTERVAL_SECS: u64 = 900;
const DEFAULT_HISTORY_CYCLES: usize = 4;
const DEFAULT_TEST_PAYLOAD_BYTES: usize = 250_000;

/// Scheduling, history and test payload limits for connection-quality reports.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub interval: Duration,
    pub history_cycles: usize,
    pub test_payload_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(DEFAULT_INTERVAL_SECS),
            history_cycles: DEFAULT_HISTORY_CYCLES,
            test_payload_bytes: DEFAULT_TEST_PAYLOAD_BYTES,
        }
    }
}

pub struct Args {
    pub dbus: zbus::Connection,
    pub zsender: zenorb::Sender,
}

#[inject(speed_test: &SpeedTest, default_cfg: &Config, zenoh: &Zenorb)]
pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    let mut cfg = default_cfg.clone();
    let cfg_sub = zenoh
        .declare_subscriber("core/config")
        .await
        .map_err(|e| eyre!("{e}"))
        .inspect_err(|e| {
            warn!("conn_quality failed to subscribe to core/config with: {e}")
        })?;

    let mut history = VecDeque::new();

    let mut interval = time::interval_at(Instant::now() + cfg.interval, cfg.interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        if interval.period() != cfg.interval {
            interval = time::interval_at(Instant::now() + cfg.interval, cfg.interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        }

        select! {
            biased;

            cfg_result = recv_config(&cfg_sub) => {
                cfg = match cfg_result  {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("core/config recv failed with: {e}");
                        continue;
                    }
                };
            }

            _ = interval.tick() => {
                let supervisor = match SupervisorProxy::new(&ctx.dbus).await {
                    Ok(proxy) => proxy,
                    Err(err) => {
                        warn!("failed to create supervisor proxy: {err}");
                        continue;
                    }
                };

                let background_downloads_allowed = time::timeout(
                    Duration::from_secs(5),
                    supervisor.background_downloads_allowed(),
                )
                .await
                .wrap_err("failed to read background-download permissions")
                .and_then(|result| {
                    result.wrap_err("timed out reading background-download permissions")
                });

                match background_downloads_allowed {
                    Ok(true) => (),

                    Ok(false) => {
                        warn!(
                            "skipping conn quality check, background downloads are not allowed"
                        );
                        continue;
                    }

                    Err(e) => {
                        warn!("{e}");
                        continue;
                    }
                };

                let cloudflare = match speed_test.run_speed_test(cfg.test_payload_bytes).await {
                    Ok(result) => result,
                    Err(err) => {
                        warn!("Cloudflare speed test failed: {err}");
                        continue;
                    }
                };

                let pcp = match speed_test
                    .run_pcp_speed_test(cfg.test_payload_bytes, 1)
                    .await
                {
                    Ok(result) => result,
                    Err(err) => {
                        warn!("PCP speed test failed: {err}");
                        continue;
                    }
                };

                let measured_at = Utc::now().timestamp_millis();
                let cycle = Cycle {
                    pcp_upload: Traffic {
                        measured_at,
                        duration_ms: pcp.upload_duration_ms,
                        bytes: (pcp.upload_mb * 1_000_000.0).round() as u64,
                        mbps: pcp.upload_mbps,
                    },
                    speed_test_upload: Traffic {
                        measured_at,
                        duration_ms: cloudflare.upload_duration_ms,
                        bytes: (cloudflare.upload_mb * 1_000_000.0).round() as u64,
                        mbps: cloudflare.upload_mbps,
                    },
                    speed_test_download: Traffic {
                        measured_at,
                        duration_ms: cloudflare.download_duration_ms,
                        bytes: (cloudflare.download_mb * 1_000_000.0).round() as u64,
                        mbps: cloudflare.download_mbps,
                    },
                };

                while history.len() >= cfg.history_cycles {
                    history.pop_front();
                }

                history.push_back(cycle);

                let payload = serde_json::to_vec(&aggregate(&history))?;

                _ = ctx
                    .zsender
                    .publisher("oes/connection_quality")?
                    .put(&payload)
                    .await
                    .inspect_err(|err| {
                        warn!("failed to publish connection-quality report: {err}")
                    });
            }
        }
    }
}

#[derive(Deserialize)]
struct CoreConfig {
    conn_quality: Option<ConnQualityConfig>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ConnQualityConfig {
    interval_secs: Option<u64>,
    history_cycles: Option<usize>,
    test_payload_bytes: Option<usize>,
}

async fn recv_config(sub: &Subscriber<FifoChannelHandler<Sample>>) -> Result<Config> {
    let sample = sub.recv_async().await.map_err(|e| eyre!("{e}"))?;
    let payload = sample.payload();
    let core: CoreConfig = serde_json::from_slice(payload.to_bytes().as_bytes())?;
    let quality = core.conn_quality.unwrap_or_default();

    let cfg = Config {
        interval: Duration::from_secs(
            quality.interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS),
        ),
        history_cycles: quality.history_cycles.unwrap_or(DEFAULT_HISTORY_CYCLES),
        test_payload_bytes: quality
            .test_payload_bytes
            .unwrap_or(DEFAULT_TEST_PAYLOAD_BYTES),
    };

    if cfg.interval.is_zero() || cfg.history_cycles == 0 || cfg.test_payload_bytes == 0
    {
        return Err(eyre!(
            "connection-quality settings must be greater than zero"
        ));
    }

    if Instant::now().checked_add(cfg.interval).is_none() {
        return Err(eyre!("connection-quality interval is too large"));
    }

    Ok(cfg)
}

/// Calls the Cloudflare and PCP speed tests using this Orb's identity and bus.
#[cfg_attr(feature = "testing", faux::create)]
pub struct SpeedTest {
    orb_id: OrbId,
    dbus: zbus::Connection,
}

#[cfg_attr(feature = "testing", faux::methods)]
impl SpeedTest {
    /// Creates a speed-test caller for this Orb.
    pub fn new(orb_id: OrbId, dbus: zbus::Connection) -> Self {
        Self { orb_id, dbus }
    }

    /// Measures Cloudflare upload and download throughput.
    pub async fn run_speed_test(&self, size_bytes: usize) -> Result<SpeedTestResults> {
        run_speed_test(size_bytes).await
    }

    /// Measures PCP upload throughput.
    pub async fn run_pcp_speed_test(
        &self,
        size_bytes: usize,
        uploads: usize,
    ) -> Result<PcpSpeedTestResults> {
        run_pcp_speed_test(size_bytes, &self.orb_id, &self.dbus, uploads).await
    }
}

struct Cycle {
    pcp_upload: Traffic,
    speed_test_upload: Traffic,
    speed_test_download: Traffic,
}

fn aggregate(history: &VecDeque<Cycle>) -> ConnectionQualityReport {
    let count = history.len() as f64;

    let average_pcp_upload_mbps =
        history.iter().map(|c| c.pcp_upload.mbps).sum::<f64>() / count;

    let average_speed_test_upload_mbps = history
        .iter()
        .map(|c| c.speed_test_upload.mbps)
        .sum::<f64>()
        / count;

    let average_speed_test_download_mbps = history
        .iter()
        .map(|c| c.speed_test_download.mbps)
        .sum::<f64>()
        / count;

    let slowest = average_pcp_upload_mbps
        .min(average_speed_test_upload_mbps)
        .min(average_speed_test_download_mbps);

    let quality = match assess_connectivity_quality(slowest) {
        ConnectivityQuality::Excellent => Quality::Excellent,
        ConnectivityQuality::Good => Quality::Good,
        ConnectivityQuality::Typical => Quality::Typical,
        ConnectivityQuality::Poor => Quality::Poor,
        ConnectivityQuality::Worst => Quality::Worst,
    };

    ConnectionQualityReport {
        pcp_upload: history.iter().map(|c| c.pcp_upload.clone()).collect(),
        speed_test_upload: history
            .iter()
            .map(|c| c.speed_test_upload.clone())
            .collect(),
        speed_test_download: history
            .iter()
            .map(|c| c.speed_test_download.clone())
            .collect(),
        quality,
        average_pcp_upload_mbps,
        average_speed_test_upload_mbps,
        average_speed_test_download_mbps,
    }
}

#[zbus::proxy(
    default_service = "org.worldcoin.OrbSupervisor1",
    default_path = "/org/worldcoin/OrbSupervisor1/Manager",
    interface = "org.worldcoin.OrbSupervisor1.Manager"
)]
trait Supervisor {
    #[zbus(property)]
    fn background_downloads_allowed(&self) -> zbus::Result<bool>;
}
