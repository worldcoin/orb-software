use crate::engine::{Engine, QrScanSchema, SignupFailReason};
use eyre::Result;
use std::time::Duration;
use tokio::time;
use tracing::info;

pub async fn signup_simulation(ui: &dyn Engine, self_serve: bool) -> Result<()> {
    info!("🔹 Starting simulation (self-serve: {})", self_serve);

    ui.bootup();
    time::sleep(Duration::from_secs(5)).await;
    ui.boot_complete(false);
    time::sleep(Duration::from_secs(1)).await;
    ui.idle();
    ui.battery_capacity(100);
    ui.good_internet();
    ui.good_wlan();
    time::sleep(Duration::from_secs(5)).await;

    if !self_serve {
        // operator presses the button to initiate signup
        ui.signup_start();
        time::sleep(Duration::from_secs(1)).await;

        ui.qr_scan_start(QrScanSchema::Operator);
        time::sleep(Duration::from_secs(4)).await;
        ui.qr_scan_completed(QrScanSchema::Operator);

        ui.qr_scan_success(QrScanSchema::Operator);
        time::sleep(Duration::from_secs(1)).await;
        ui.qr_scan_start(QrScanSchema::User);
        time::sleep(Duration::from_secs(4)).await;
        ui.qr_scan_completed(QrScanSchema::User);

        ui.qr_scan_success(QrScanSchema::User);
    }

    // biometric capture start, either:
    // - cone button pressed, or
    // - app button pressed
    ui.biometric_capture_start();
    time::sleep(Duration::from_secs(2)).await;

    // waiting for the user to be in correct position
    ui.biometric_capture_distance(false);
    time::sleep(Duration::from_secs(4)).await;

    // user is in correct position
    ui.biometric_capture_distance(true);
    ui.biometric_capture_occlusion(false);
    for i in 0..10 {
        if (4..=6).contains(&i) {
            // simulate user moving away
            ui.biometric_capture_distance(false);
            ui.biometric_capture_occlusion(true);
        } else {
            // capture is making progress
            ui.biometric_capture_distance(true);
            ui.biometric_capture_occlusion(false);
            ui.biometric_capture_distance(true);
            ui.biometric_capture_progress(i as f64 / 10.0);
        }

        time::sleep(Duration::from_secs(1)).await;
    }
    // fill the ring
    ui.biometric_capture_progress(1.1);
    time::sleep(Duration::from_secs(1)).await;

    ui.biometric_capture_success();

    if !self_serve {
        // biometric pipeline, in 2 stages
        // to test `starting_enrollment`
        time::sleep(Duration::from_secs(1)).await;
        for i in 0..5 {
            ui.biometric_pipeline_progress(i as f64 / 10.0);
            time::sleep(Duration::from_secs(1)).await;
        }
        ui.starting_enrollment();
        time::sleep(Duration::from_secs(4)).await;
        for i in 5..10 {
            ui.biometric_pipeline_progress(i as f64 / 10.0);
            time::sleep(Duration::from_millis(500)).await;
        }
        ui.biometric_pipeline_success();

        time::sleep(Duration::from_secs(1)).await;
        if rand::random::<u8>() % 2 == 0 {
            ui.signup_success();
        } else {
            let fail_reason = SignupFailReason::from(
                rand::random::<u8>() % SignupFailReason::Unknown as u8,
            );
            ui.signup_fail(fail_reason);
        }
    }

    ui.idle();
    time::sleep(Duration::from_secs(7)).await;

    ui.shutdown(true);
    time::sleep(Duration::from_secs(2)).await;

    Ok(())
}
