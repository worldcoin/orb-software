use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

use color_eyre::{eyre::eyre, Result};
use orb_info::OrbId;
use seek_camera::{camera::Camera, frame_format::FrameFormat};
use serde::Serialize;
use tracing::{info, warn};

const ZENOH_PORT: u16 = 7447;
const PAIRING_KEY: &str = "hardware/status/thermal_camera_pairing";
const CALIBRATION_KEY: &str = "hardware/status/thermal_camera_calibration";
const FRAME_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize)]
struct HardwareState {
    status: String,
    message: String,
}

pub fn verify_and_publish_pairing(cam: &mut Camera, orb_id: &OrbId) -> Result<()> {
    let verification = verify_camera(cam);
    let (status, message) = match &verification {
        Ok(()) => ("success", "paired and verified".to_string()),
        Err(e) => {
            warn!("Thermal camera pairing verification failed: {e}");
            ("failure", format!("verification failed: {e}"))
        }
    };

    if let Err(e) = publish(orb_id, PAIRING_KEY, status, &message) {
        warn!("Failed to publish thermal camera pairing status: {e}");
    }

    verification
}

pub fn publish_pairing_failure(orb_id: &OrbId, message: &str) {
    if let Err(e) = publish(orb_id, PAIRING_KEY, "failure", message) {
        warn!("Failed to publish thermal camera pairing failure: {e}");
    }
}

pub fn publish_calibration_status(orb_id: &OrbId, status: &str, message: &str) {
    if let Err(e) = publish(orb_id, CALIBRATION_KEY, status, message) {
        warn!("Failed to publish thermal camera calibration status: {e}");
    }
}

fn publish(orb_id: &OrbId, key: &str, status: &str, message: &str) -> Result<()> {
    let state = HardwareState {
        status: status.to_string(),
        message: message.to_string(),
    };
    let payload = serde_json::to_string(&state)?;
    let keyexpr = format!("{}/{key}", orb_id);

    info!(
        "Publishing thermal camera health to {key}: status={status}, message={message}"
    );

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let session = zenorb::zenoh::open(zenorb::client_cfg(ZENOH_PORT))
            .await
            .map_err(|e| eyre!("failed to open zenoh session: {e}"))?;

        session
            .put(&keyexpr, payload)
            .await
            .map_err(|e| eyre!("failed to publish to zenoh: {e}"))?;

        Ok(())
    })
}

fn verify_camera(cam: &mut Camera) -> Result<()> {
    let (tx, rx) = mpsc::sync_channel::<()>(1);
    let frame_seen = Arc::new(AtomicBool::new(false));
    let frame_seen_in_cb = frame_seen.clone();
    cam.set_callback(Box::new(move |_frame| {
        if !frame_seen_in_cb.swap(true, Ordering::AcqRel) {
            let _ = tx.try_send(());
        }
    }))
    .map_err(|e| eyre!("failed to set camera callback: {e}"))?;

    cam.capture_session_start(FrameFormat::Grayscale)
        .map_err(|e| eyre!("failed to start capture session: {e}"))?;

    let result = rx.recv_timeout(FRAME_TIMEOUT);

    cam.capture_session_stop()
        .map_err(|e| eyre!("failed to stop capture session: {e}"))?;

    result.map_err(|_| eyre!("timed out waiting for thermal camera frame"))?;

    info!("Thermal camera verification succeeded: received frame");

    Ok(())
}
