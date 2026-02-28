use std::{
    ffi::{CStr, CString},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, OnceLock,
    },
    time::Duration,
};

use clap::{Args, Subcommand};
use color_eyre::{
    eyre::{eyre, WrapErr},
    owo_colors::{AnsiColors, OwoColorize},
    Result,
};
use indicatif::ProgressBar;
use orb_info::{orb_os_release::OrbOsPlatform, OrbId};
use seek_camera::manager::{CameraHandle, Event, Manager};
use std::process::Command;
use tracing::{info, warn};

use crate::{health, start_manager, Flow};

const DEFAULT_PAIRING_TIMEOUT_SECS: u64 = 90;

/// Manages pairing of the camera
#[derive(Debug, Args)]
pub struct Pairing {
    #[clap(subcommand)]
    commands: Commands,
}

impl Pairing {
    pub fn run(self, platform: OrbOsPlatform, orb_id: Option<&OrbId>) -> Result<()> {
        match self.commands {
            Commands::Status(c) => c.run(),
            Commands::Pair(c) => c.run(platform, orb_id),
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    Status(Status),
    Pair(Pair),
}

/// Checks the pairing status
#[derive(Debug, Args)]
struct Status {
    /// Continue to check for new camera events even after the first one.
    #[clap(short)]
    continue_running: bool,
}

impl Status {
    fn run(self) -> Result<()> {
        let cam_fn = move |mngr: &mut _, cam_h, evt, _err| {
            helper(
                mngr,
                cam_h,
                evt,
                PairingBehavior::DoNothing,
                self.continue_running,
                None,
                None,
            )
        };
        start_manager(Box::new(cam_fn), None)
    }
}

/// Pairs camera(s)
#[derive(Debug, Args)]
struct Pair {
    /// Forces cameras already paired to re-pair.
    #[clap(short)]
    force_pair: bool,
    /// Continue to check for new camera events even after the first one.
    #[clap(short)]
    continue_running: bool,
    #[clap(long)]
    from_dir: Option<PathBuf>,
    /// Timeout in seconds for waiting for a camera event. Exits with failure
    /// if no camera is detected within this duration.
    #[clap(long, default_value_t = DEFAULT_PAIRING_TIMEOUT_SECS)]
    timeout_secs: u64,
}

impl Pair {
    fn run(self, platform: OrbOsPlatform, orb_id: Option<&OrbId>) -> Result<()> {
        power_cycle_heat_camera(platform)?;
        let continue_running = self.continue_running;
        let timeout_secs = self.timeout_secs;

        let from_dir = self
            .from_dir
            .map(|p| CString::new(p.to_string_lossy().into_owned()).unwrap());
        let pairing_behavior = if self.force_pair {
            PairingBehavior::ForcePair
        } else {
            PairingBehavior::Pair
        };
        let timeout = Duration::from_secs(timeout_secs);
        let orb_id_owned = orb_id.cloned();
        let camera_detected = Arc::new(AtomicBool::new(false));
        let camera_detected_in_cb = camera_detected.clone();
        let orb_id_for_usb_status = orb_id.cloned();

        let (watchdog_cancel_send, watchdog) = if continue_running {
            (None, None)
        } else {
            let (watchdog_cancel_send, watchdog_cancel_recv) = mpsc::channel::<()>();
            let watchdog = {
                let orb_id = orb_id.cloned();
                std::thread::spawn(move || {
                    match watchdog_cancel_recv.recv_timeout(timeout) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    tracing::error!(
                        "Pairing timed out after {timeout_secs}s, force exiting"
                    );
                    if let Some(orb_id) = &orb_id {
                        health::publish_usb_failure(
                            orb_id,
                            &format!(
                                "timed out waiting for thermal camera detection after {timeout_secs}s"
                            ),
                        );
                        health::publish_pairing_failure(
                            orb_id,
                            &format!("pairing timed out after {timeout_secs}s"),
                        );
                    }
                    std::process::exit(1);
                })
            };

            (Some(watchdog_cancel_send), Some(watchdog))
        };

        let mut watchdog_cancel_on_detect = watchdog_cancel_send.clone();
        let cam_fn = move |mngr: &mut _, cam_h, evt, _err| {
            if is_camera_detected_event(&evt) {
                if !camera_detected_in_cb.swap(true, Ordering::AcqRel) {
                    if let Some(orb_id) = orb_id_for_usb_status.as_ref() {
                        health::publish_usb_status(
                            orb_id,
                            "success",
                            "thermal camera detected by seek manager",
                        );
                    }
                    if let Some(watchdog_cancel_send) = watchdog_cancel_on_detect.take()
                    {
                        let _ = watchdog_cancel_send.send(());
                    }
                }
            }

            helper(
                mngr,
                cam_h,
                evt,
                pairing_behavior,
                continue_running,
                from_dir.as_deref(),
                orb_id_owned.as_ref(),
            )
        };

        let result = start_manager(Box::new(cam_fn), Some(timeout));
        if let Some(watchdog_cancel_send) = watchdog_cancel_send {
            let _ = watchdog_cancel_send.send(());
        }

        if let Err(e) = &result {
            warn!("Pairing failed: {e}");
            if let Some(orb_id) = orb_id {
                if !camera_detected.load(Ordering::Acquire) {
                    health::publish_usb_failure(
                        orb_id,
                        &format!("thermal camera was not detected: {e}"),
                    );
                }
                health::publish_pairing_failure(
                    orb_id,
                    &format!("pairing service failed: {e}"),
                );
            }
        }

        if let Some(watchdog) = watchdog {
            let _ = watchdog.join();
        }

        result
    }
}

///////////////////////////
// ---- Helper Code ---- //
///////////////////////////
fn power_cycle_heat_camera(platform: OrbOsPlatform) -> Result<()> {
    match platform {
        OrbOsPlatform::Diamond => {
            info!("Power-cycling heat camera (2v8 line) using orb-mcu-util (Diamond platform)");

            let status = Command::new("orb-mcu-util")
                .args(["power-cycle", "heat-camera"])
                .status()
                .wrap_err("Failed to execute orb-mcu-util power-cycle heat-camera")?;
            if !status.success() {
                return Err(eyre!(
                    "orb-mcu-util power-cycle heat-camera exited with non-zero status: {status}"));
            }
        }
        OrbOsPlatform::Pearl => {}
    }

    Ok(())
}

fn is_camera_detected_event(evt: &Event) -> bool {
    matches!(evt, Event::Connect | Event::ReadyToPair)
}

/// Used to control the pairing behavior of [`helper`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum PairingBehavior {
    DoNothing,
    /// Pairs cameras that are not yet paired.
    Pair,
    /// Pairs all cameras, even if they were already paired.
    ForcePair,
}

/// Because [`Status`] and [`Pair`] have such similar logic, we use this function
/// to easily reuse code.
fn helper(
    mngr: &mut Manager,
    cam_h: CameraHandle,
    evt: Event,
    pairing_behavior: PairingBehavior,
    continue_running: bool,
    from_dir: Option<&CStr>,
    orb_id: Option<&OrbId>,
) -> Result<Flow> {
    let is_paired = match evt {
        Event::Connect => true,
        Event::Disconnect => return Ok(Flow::Continue),
        Event::ReadyToPair => false,
        Event::Error => return Ok(Flow::Continue),
    };
    let mut cams = mngr.cameras().unwrap();
    let cam = cams
        .get_mut(&cam_h)
        .ok_or_else(|| eyre!("failed to get camera from handle"))?;

    let serial = cam
        .serial_number()
        .wrap_err("Failed to get serial number")?;
    let cid = cam.chip_id().wrap_err("Failed to get chip id")?;

    let paired = if is_paired {
        "paired".color(AnsiColors::Green)
    } else {
        "unpaired".color(AnsiColors::Red)
    };
    info!("Found {paired} camera with cid: {cid}, serial: {serial}");

    if pairing_behavior == PairingBehavior::ForcePair
        || pairing_behavior == PairingBehavior::Pair && !is_paired
    {
        info!("Pairing camera (cid {cid})...");
        cam.store_calibration_data(from_dir, Some(pair_progress_cb))
            .wrap_err("Error while pairing camera")?;
        info!("{} camera (cid {cid})", "Paired".green());
    }

    if let Some(orb_id) = orb_id {
        health::verify_and_publish_pairing(cam, orb_id)
            .wrap_err("Thermal camera pairing verification failed")?;
    }

    if continue_running {
        Ok(Flow::Continue)
    } else {
        Ok(Flow::Finish)
    }
}

fn pair_progress_cb(pct: u8) {
    static BAR: OnceLock<ProgressBar> = OnceLock::new();
    BAR.get_or_init(|| ProgressBar::new(100))
        .set_position(pct as u64);
}
