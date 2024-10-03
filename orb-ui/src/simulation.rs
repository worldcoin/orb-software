use crate::engine::{Engine, QrScanSchema, SignupFailReason};
use crate::Hardware;
use eyre::Result;
use rand::distributions::{Distribution, Standard};
use rand::Rng;
use std::time::Duration;
use tokio::{fs, time};
use tracing::{error, info};

/// Implement rand::random::<SignupFailReason>()
impl Distribution<SignupFailReason> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> SignupFailReason {
        match rng.gen_range(0..=8) {
            0 => SignupFailReason::Timeout,
            1 => SignupFailReason::FaceNotFound,
            2 => SignupFailReason::Duplicate,
            3 => SignupFailReason::Server,
            4 => SignupFailReason::Verification,
            5 => SignupFailReason::SoftwareVersionDeprecated,
            6 => SignupFailReason::SoftwareVersionBlocked,
            7 => SignupFailReason::UploadCustodyImages,
            _ => SignupFailReason::Unknown,
        }
    }
}

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
    hardware: Hardware,
    self_serve: bool,
    showcar: bool,
) -> Result<()> {
    info!("🔹 Starting signup simulation (self-serve: {})", self_serve);

    ui.battery_capacity(100);
    ui.good_internet();
    ui.good_wlan();
    ui.idle();
    if hardware == Hardware::Diamond && self_serve || showcar {
        // idle state is waiting for user QR code
        ui.qr_scan_start(QrScanSchema::User);
    }
    time::sleep(Duration::from_secs(1)).await;

    // gimbal facing the user as much as possible
    ui.gimbal(1, 90000);

    time::sleep(Duration::from_secs(5)).await;

    if !self_serve {
        // operator presses the button to initiate signup
        ui.signup_start_operator();
        time::sleep(Duration::from_secs(1)).await;
    }

    loop {
        if !showcar {
            // scanning operator QR code
            ui.qr_scan_start(QrScanSchema::Operator);
            time::sleep(Duration::from_secs(10)).await;
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
        }

        info!("Waiting for button press");
        if showcar {
            // check that file exists
            if !std::path::Path::new("/sys/class/gpio/PQ.06/value").exists() {
                // write 441 into /sys/class/gpio/export
                if let Err(e) = fs::write("/sys/class/gpio/export", "441").await {
                    error!("Button cannot be configured, use `sudo echo 441 > /sys/class/gpio/export`: {}", e);
                }
                break;
            }

            loop {
                // read cat /sys/class/gpio/PQ.06/value in a loop until it's 1
                if let Ok(value) =
                    fs::read_to_string("/sys/class/gpio/PQ.06/value").await
                {
                    if value.trim() == "1" {
                        break;
                    }
                    time::sleep(Duration::from_millis(100)).await;
                }
            }
        } else if self_serve {
            // waiting for app button press
            time::sleep(Duration::from_secs(6)).await;
        }
        info!("Starting capture");

        // biometric capture start, either:
        // - cone button pressed, or
        // - app button pressed
        ui.signup_start();

        let mut x_angle = 1_i32;
        if showcar {
            time::sleep(Duration::from_secs(2)).await;
            let steps = 2000 / 30_u32; // 30ms per step, 200ms total
            let gimbal_x_steps = 45000_u32 / steps;
            for i in 0..steps {
                ui.gimbal(gimbal_x_steps * i, 90000);
                time::sleep(Duration::from_millis(30)).await;
            }
        }

        // waiting for the user to be in correct position
        ui.biometric_capture_distance(false);
        time::sleep(Duration::from_millis(8500)).await;

        let mut biometric_capture_error = false;

        // 100 steps, 80ms per step, 8 seconds total
        let biometric_capture_interval_ms = 70;
        for i in 1..=100 {
            if !showcar && (30..=50).contains(&i) {
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
            if !showcar && i == 50 && rand::random::<u8>() % 5 == 0 {
                info!("⚠️ Simulating biometric capture error");
                biometric_capture_error = true;
                ui.signup_fail(SignupFailReason::Timeout);
                break;
            }

            // simulate gimbal movement
            let x_base = if i < 50 { 40000_i32 } else { 35000_i32 };
            let x_rand = rand::random::<i32>() % 1000_i32;
            let y_rand = rand::random::<i32>() % 1000_i32;
            x_angle = x_base + x_rand;
            ui.gimbal(x_angle as u32, (90000_i32 + y_rand) as u32);

            time::sleep(Duration::from_millis(biometric_capture_interval_ms)).await;
        }

        if !biometric_capture_error {
            // fill the ring
            ui.biometric_capture_progress(1.1);
            time::sleep(Duration::from_secs(1)).await;

            ui.biometric_capture_success();

            if showcar {
                time::sleep(Duration::from_secs(4)).await;
            }

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
                    let fail_reason = rand::random::<SignupFailReason>();
                    ui.signup_fail(fail_reason);
                }
            }
        }

        let steps: i32 = 2000_i32 / 30_i32; // 30ms per step, 200ms total
        let gimbal_x_steps: i32 = 45000_i32 / steps;
        while x_angle > 0 {
            ui.gimbal(x_angle as u32, 90000);
            x_angle -= gimbal_x_steps;
            time::sleep(Duration::from_millis(30)).await;
        }

        time::sleep(Duration::from_millis(5000)).await;
        // back to idle
        ui.idle();
        if hardware == Hardware::Diamond && self_serve || showcar {
            // idle state is waiting for user QR code
            ui.qr_scan_start(QrScanSchema::User);
        }

        if !showcar {
            // wait for sound etc to finish
            time::sleep(Duration::from_millis(5000)).await;
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
