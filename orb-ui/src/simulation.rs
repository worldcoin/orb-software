use crate::engine::{Engine, QrScanSchema, SignupFailReason};
use eyre::Result;
use std::time::Duration;
use tokio::time;
use tracing::info;

#[expect(dead_code)]
pub async fn bootup_simulation(ui: &dyn Engine) -> Result<()> {
    info!("🔹 Starting boot-up simulation");

    ui.bootup();
    time::sleep(Duration::from_secs(1)).await;
    ui.boot_complete(false);
    time::sleep(Duration::from_secs(1)).await;
    ui.idle();
    ui.battery_capacity(100);
    ui.good_internet();
    ui.good_wlan();
    time::sleep(Duration::from_secs(2)).await;

    Ok(())
}

pub async fn signup_simulation(
    ui: &dyn Engine,
    self_serve: bool,
    looping: bool,
) -> Result<()> {
    info!("🔹 Starting signup simulation (self-serve: {})", self_serve);

    ui.idle();
    time::sleep(Duration::from_secs(1)).await;

    if !self_serve {
        // operator presses the button to initiate signup
        ui.signup_start();
        time::sleep(Duration::from_secs(1)).await;
    }

    loop {
        // scanning operator QR code
        ui.qr_scan_start(QrScanSchema::Operator);
        time::sleep(Duration::from_secs(4)).await;
        ui.qr_scan_capture();
        time::sleep(Duration::from_secs(2)).await;
        ui.qr_scan_completed(QrScanSchema::Operator);
        ui.qr_scan_success(QrScanSchema::Operator);

        // scanning user QR code
        time::sleep(Duration::from_secs(1)).await;
        ui.qr_scan_start(QrScanSchema::User);
        time::sleep(Duration::from_secs(4)).await;
        ui.qr_scan_capture();
        time::sleep(Duration::from_secs(2)).await;
        ui.qr_scan_completed(QrScanSchema::User);
        ui.qr_scan_success(QrScanSchema::User);

        // biometric capture start, either:
        // - cone button pressed, or
        // - app button pressed
        ui.biometric_capture_start();
        time::sleep(Duration::from_secs(1)).await;

        // waiting for the user to be in correct position
        ui.biometric_capture_distance(false);
        time::sleep(Duration::from_secs(6)).await;

        let mut biometric_capture_error = false;

        // user is in correct position
        ui.biometric_capture_distance(true);
        ui.biometric_capture_occlusion(false);
        for i in 0..100 {
            if (30..=50).contains(&i) {
                // simulate user moving away
                ui.biometric_capture_distance(false);
                ui.biometric_capture_occlusion(true);
            } else {
                // capture is making progress
                ui.biometric_capture_distance(true);
                ui.biometric_capture_occlusion(false);
                ui.biometric_capture_distance(true);
                ui.biometric_capture_progress(i as f64 / 100.0);
            }

            // randomly simulate error
            if i == 50 && rand::random::<u8>() % 5 == 0 {
                info!("⚠️ Simulating biometric capture error");
                biometric_capture_error = true;
                ui.signup_fail(SignupFailReason::Timeout);
                break;
            }

            // simulate gimbal movement
            let x_base = if i < 50 { 47000 } else { 43000 };
            let x_rand = rand::random::<i32>() % 500;
            let y_rand = rand::random::<i32>() % 500;
            let x_sign: i32 = if rand::random::<u8>() % 2 == 0 { 1 } else { -1 };
            let y_sign: i32 = if rand::random::<u8>() % 2 == 0 { 1 } else { -1 };
            ui.gimbal(
                x_base + (x_rand * x_sign) as u32,
                90000 + (y_rand * y_sign) as u32,
            );

            time::sleep(Duration::from_millis(100)).await;
        }

        if !biometric_capture_error {
            // fill the ring
            ui.biometric_capture_progress(1.1);
            time::sleep(Duration::from_secs(1)).await;

            ui.biometric_capture_success();

            if !self_serve {
                // biometric pipeline, in 2 stages
                // to test `starting_enrollment`
                time::sleep(Duration::from_secs(1)).await;
                for i in 0..=2 {
                    ui.biometric_pipeline_progress(i as f64 / 5.0);
                    time::sleep(Duration::from_secs(1)).await;
                }
                time::sleep(Duration::from_secs(1)).await;
                ui.starting_enrollment();
                time::sleep(Duration::from_secs(2)).await;
                for i in 2..5 {
                    ui.biometric_pipeline_progress(i as f64 / 5.0);
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
        }

        ui.idle();
        time::sleep(Duration::from_secs(20)).await;

        if !looping {
            break;
        }
    }

    Ok(())
}

#[expect(dead_code)]
pub async fn shutdown_simulation(ui: &dyn Engine) -> Result<()> {
    ui.shutdown(true);
    time::sleep(Duration::from_secs(2)).await;

    Ok(())
}
