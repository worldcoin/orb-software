//! Measures connection quality and reports it through OES.

use color_eyre::Result;
use crabwire::inject;
use oes::{ConnectionQualityReport, Quality, Traffic};
use orb_info::OrbId;
use orb_speed_test::{
    assess_connectivity_quality, run_pcp_speed_test, run_speed_test,
    ConnectivityQuality, PcpSpeedTestResults, SpeedTestResults,
};
use speare::mini;
use std::{collections::VecDeque, time::Duration};
use tokio::time::{self, Instant, MissedTickBehavior};
use tracing::warn;

/// Scheduling and history limits for connection-quality reports.
pub struct Config {
    pub interval: Duration,
    pub max_reports: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(3600),
            max_reports: 4,
        }
    }
}

pub struct Args {
    pub dbus: zbus::Connection,
    pub zsender: zenorb::Sender,
}

#[inject(speed_test: &SpeedTest, config: &Config)]
pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    let mut history = VecDeque::new();

    let mut interval =
        time::interval_at(Instant::now() + config.interval, config.interval);

    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        let supervisor = match SupervisorProxy::new(&ctx.dbus).await {
            Ok(proxy) => proxy,
            Err(err) => {
                warn!("failed to create supervisor proxy: {err}");
                continue;
            }
        };

        match time::timeout(
            Duration::from_secs(5),
            supervisor.background_downloads_allowed(),
        )
        .await
        {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => continue,
            Ok(Err(err)) => {
                warn!("failed to read background-download permission: {err}");
                continue;
            }
            Err(err) => {
                warn!("timed out reading background-download permission: {err}");
                continue;
            }
        }

        let cloudflare = match speed_test.run_speed_test(500_000).await {
            Ok(result) => result,
            Err(err) => {
                warn!("Cloudflare speed test failed: {err}");
                continue;
            }
        };

        let pcp = match speed_test.run_pcp_speed_test(500_000, 1).await {
            Ok(result) => result,
            Err(err) => {
                warn!("PCP speed test failed: {err}");
                continue;
            }
        };

        let measured_at = chrono::Utc::now().timestamp_millis();
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
                bytes: 1_000_000,
                mbps: cloudflare.upload_mbps,
            },
            speed_test_download: Traffic {
                measured_at,
                duration_ms: cloudflare.download_duration_ms,
                bytes: 1_000_000,
                mbps: cloudflare.download_mbps,
            },
        };

        if history.len() == config.max_reports {
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
