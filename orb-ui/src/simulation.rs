use crate::engine::{Engine, QrScanSchema};
use crate::sound;
use crate::sound::Player;
use eyre::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time;
use tracing::info;

pub async fn simulate(ui: &dyn Engine, sound: Arc<Mutex<sound::Jetson>>) -> Result<()> {
    info!("🔹 Starting simulation");
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::BootUp))?;

    ui.bootup();
    time::sleep(Duration::from_secs(5)).await;
    ui.boot_complete();
    time::sleep(Duration::from_secs(1)).await;
    ui.idle();
    ui.battery_capacity(100);
    ui.good_internet();
    ui.good_wlan();
    time::sleep(Duration::from_secs(5)).await;
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::StartSignup))?;

    ui.signup_start();
    time::sleep(Duration::from_secs(2)).await;
    ui.qr_scan_start(QrScanSchema::Operator);
    time::sleep(Duration::from_secs(4)).await;
    ui.qr_scan_completed(QrScanSchema::Operator);
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::QrLoadSuccess))?;

    ui.qr_scan_success(QrScanSchema::Operator);
    time::sleep(Duration::from_secs(1)).await;
    ui.qr_scan_start(QrScanSchema::User);
    time::sleep(Duration::from_secs(4)).await;
    ui.qr_scan_completed(QrScanSchema::User);
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::UserQrLoadSuccess))?;

    ui.qr_scan_success(QrScanSchema::User);
    time::sleep(Duration::from_secs(1)).await;

    ui.biometric_capture_occlusion(true);
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::IrisScanningLoop01A))?;

    time::sleep(Duration::from_secs(2)).await;
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::IrisScanningLoop01A))?;

    time::sleep(Duration::from_secs(2)).await;
    ui.biometric_capture_occlusion(false);
    for i in 0..5 {
        match i % 6 {
            0 => sound
                .lock()
                .await
                .play(sound::Type::Melody(sound::Melody::IrisScanningLoop01A))?,
            1 => sound
                .lock()
                .await
                .play(sound::Type::Melody(sound::Melody::IrisScanningLoop01B))?,
            2 => sound
                .lock()
                .await
                .play(sound::Type::Melody(sound::Melody::IrisScanningLoop01C))?,
            3 => sound
                .lock()
                .await
                .play(sound::Type::Melody(sound::Melody::IrisScanningLoop02A))?,
            4 => sound
                .lock()
                .await
                .play(sound::Type::Melody(sound::Melody::IrisScanningLoop02B))?,
            5 => sound
                .lock()
                .await
                .play(sound::Type::Melody(sound::Melody::IrisScanningLoop02C))?,
            _ => {}
        }
        ui.biometric_capture_progress(i as f64 * 2.0 / 10.0);
        time::sleep(Duration::from_secs(2)).await;
    }
    ui.biometric_capture_progress(1.1);
    time::sleep(Duration::from_secs(1)).await;
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::SignupSuccess))?;

    ui.biometric_capture_success();

    time::sleep(Duration::from_secs(1)).await;
    for i in 0..10 {
        ui.biometric_pipeline_progress(i as f64 / 10.0);
        time::sleep(Duration::from_secs(1)).await;
    }
    ui.biometric_pipeline_progress(1.1);
    time::sleep(Duration::from_secs(3)).await;
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::SignupSuccess))?;

    ui.biometric_pipeline_success();

    time::sleep(Duration::from_secs(1)).await;
    ui.signup_unique();

    ui.idle();
    time::sleep(Duration::from_secs(5)).await;
    sound
        .lock()
        .await
        .play(sound::Type::Melody(sound::Melody::PoweringDown))?;

    ui.shutdown(true);
    time::sleep(Duration::from_secs(2)).await;

    Ok(())
}
