#![cfg(feature = "testing")]
//! Checks connection-quality reports published by the running daemon.

mod fixture;

use color_eyre::eyre::eyre;
use crabwire::Registry;
use faux::when;
use fixture::Fixture;
use orb_connd::reporters::conn_quality::{Config, SpeedTest};
use orb_info::orb_os_release::{OrbOsPlatform, OrbRelease};
use orb_speed_test::{ConnectivityQuality, PcpSpeedTestResults, SpeedTestResults};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn publishes_bounded_history_and_per_test_pcp_metrics() {
    // Arrange
    let cloudflare_calls = Arc::new(AtomicUsize::new(0));
    let pcp_calls = Arc::new(AtomicUsize::new(0));
    let mut speed_test = SpeedTest::faux();
    let total_cycles = 3;

    let cfg = Config {
        interval: Duration::from_secs(1),
        history_cycles: 2,
        test_payload_bytes: 750_000,
    };

    // Mock speeds and durations are independent; this tests report aggregation,
    // not speed-test calculations.
    let calls = Arc::clone(&cloudflare_calls);
    when!(speed_test.run_speed_test).then(move |size| {
        assert_eq!(size, cfg.test_payload_bytes);
        let cycle = calls.fetch_add(1, Ordering::SeqCst);

        if cycle == 0 {
            return Err(eyre!("Cloudflare test failure"));
        }

        Ok(SpeedTestResults {
            connectivity: if cycle >= 2 {
                ConnectivityQuality::Excellent
            } else {
                ConnectivityQuality::Good
            },
            upload_mbps: cycle as f64 * 10.0,
            download_mbps: cycle as f64 * 20.0,
            upload_mb: cfg.test_payload_bytes as f64 / 1_000_000.0,
            download_mb: cfg.test_payload_bytes as f64 / 1_000_000.0,
            upload_duration_ms: cycle as u64 * 50,
            download_duration_ms: cycle as u64 * 50,
        })
    });

    let calls = Arc::clone(&pcp_calls);
    when!(speed_test.run_pcp_speed_test).then(move |(size, uploads)| {
        assert_eq!((size, uploads), (cfg.test_payload_bytes, 1));
        let cycle = calls.fetch_add(1, Ordering::SeqCst);

        if cycle == 0 {
            return Err(eyre!("PCP test failure"));
        }

        Ok(PcpSpeedTestResults {
            connectivity: if cycle == 1 {
                ConnectivityQuality::Poor
            } else {
                ConnectivityQuality::Typical
            },
            upload_mbps: cycle as f64 * 0.5,
            upload_mb: cfg.test_payload_bytes as f64 / 1_000_000.0,
            upload_duration_ms: 300,
        })
    });

    let mut fixture = Fixture::platform(OrbOsPlatform::Pearl)
        .release(OrbRelease::Dev)
        .build()
        .await;

    // Act
    let handle = fixture
        .run_with()
        .registry(Registry::new().insert(speed_test))
        .call()
        .await;

    let subscriber = handle
        .zenoh()
        .declare_subscriber("connd/oes/connection_quality")
        .await
        .unwrap();

    handle
        .zenoh()
        .session()
        .put(
            format!("{}/core/config", handle.zenoh().orb_id()),
            serde_json::json!({
                "conn_quality": {
                    "IntervalSecs": cfg.interval.as_secs(),
                    "HistoryCycles": cfg.history_cycles,
                    "TestPayloadBytes": cfg.test_payload_bytes,
                }
            })
            .to_string(),
        )
        .await
        .unwrap();

    time::sleep(Duration::from_secs(2)).await;
    let blocked_cloudflare_calls = cloudflare_calls.load(Ordering::SeqCst);
    let blocked_pcp_calls = pcp_calls.load(Ordering::SeqCst);
    let blocked_metrics = handle
        .dogstatsd
        .occurrences("orb.platform.connd.pcp_upload_mbps:");

    handle.allow_background_downloads();

    let received = time::timeout(Duration::from_secs(8), async {
        let mut reports = Vec::new();
        for _ in 0..total_cycles {
            let sample = subscriber.recv_async().await.unwrap();

            reports.push(
                serde_json::from_slice::<oes::ConnectionQualityReport>(
                    &sample.payload().to_bytes(),
                )
                .unwrap(),
            );
        }

        reports
    })
    .await;

    let metrics_received = time::timeout(Duration::from_secs(1), async {
        while handle
            .dogstatsd
            .occurrences("orb.platform.connd.pcp_upload_mbps:")
            < 4
        {
            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    let metrics = handle.dogstatsd.recvd().join("\n");
    handle.stop().await;

    // Assert
    assert_eq!(
        blocked_cloudflare_calls, 0,
        "Cloudflare speed test ran while background downloads were disabled"
    );

    assert_eq!(
        blocked_pcp_calls, 0,
        "PCP speed test ran while background downloads were disabled"
    );

    assert_eq!(blocked_metrics, 0);

    metrics_received.expect("PCP metrics were not received");

    assert_eq!(
        metrics
            .matches("orb.platform.connd.pcp_upload_mbps:")
            .count(),
        4
    );

    for mbps in [0.5, 1.0, 1.5, 2.0] {
        assert_eq!(
            metrics.matches(&format!("orb.platform.connd.pcp_upload_mbps:{mbps}|d")).count(),
            1,
            "expected one raw sample per successful PCP test, including the Cloudflare failure"
        );
    }

    let reports = received.unwrap();
    let last = reports.last().unwrap();

    assert_eq!(
        last.pcp_upload.iter().map(|m| m.mbps).collect::<Vec<_>>(),
        vec![1.5, 2.0]
    );

    assert_eq!(
        last.speed_test_upload
            .iter()
            .map(|m| m.mbps)
            .collect::<Vec<_>>(),
        vec![20.0, 30.0]
    );

    assert_eq!(
        last.speed_test_download
            .iter()
            .map(|m| m.mbps)
            .collect::<Vec<_>>(),
        vec![40.0, 60.0]
    );

    assert_eq!(last.average_pcp_upload_mbps, 1.75);
    assert_eq!(last.average_speed_test_upload_mbps, 25.0);
    assert_eq!(last.average_speed_test_download_mbps, 50.0);
    assert_eq!(last.quality, oes::Quality::Typical);
}
