#![forbid(unsafe_code)]

use humantime::parse_duration;
use once_cell::sync::Lazy;
use orb_info::orb_os_release::OrbOsRelease;
use std::env;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::fs;

use clap::Parser;
use eyre::{Context, Result};
use futures::channel::mpsc;
use orb_build_info::{make_build_info, BuildInfo};
use tokio::time;
use tracing::debug;

use crate::beacon::beacon;
use crate::engine::{Engine, Event, EventChannel, OperatingMode};
use crate::observer::listen;
use crate::serial::Serial;
use crate::simulation::signup_simulation;

mod beacon;
mod dbus;
mod engine;
mod observer;
mod serial;
mod simulation;
pub mod sound;

const INPUT_CAPACITY: usize = 100;
const BUILD_INFO: BuildInfo = make_build_info!();
const SYSLOG_IDENTIFIER: &str = "worldcoin-ui";

/// Utility args
#[derive(Parser, Debug)]
#[clap(
    author,
    version=BUILD_INFO.version,
    about = "Orb UI daemon",
    long_about = "Handles the UI of the Orb, based on dbus messages"
)]
struct Args {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser, Debug)]
enum SubCommand {
    /// Orb UI daemon, listening and reacting to dbus messages
    #[clap(action)]
    Daemon,

    /// Signup simulation
    #[clap(subcommand)]
    Simulation(SimulationArgs),

    /// Beacon mode
    #[clap(action)]
    Beacon(BeaconArgs),

    /// Recovery UI
    #[clap(action)]
    Recovery,
}

#[derive(Parser, Debug, Eq, PartialEq)]
enum SimulationArgs {
    /// Self-serve signup, app-based
    #[clap(action)]
    SelfServe,

    /// Operator-based signup, with QR codes
    #[clap(action)]
    Operator,

    /// show-car, infinite loop of signup
    #[clap(action)]
    ShowCar,
}

#[derive(Parser, Debug, Eq, PartialEq)]
struct BeaconArgs {
    /// Duration for the beacon mode
    #[arg(long, default_value = "3s", value_parser = parse_duration)]
    duration: Duration,
}

fn current_release_type() -> Result<String> {
    let os_release = OrbOsRelease::read_blocking().wrap_err("failed reading /etc/os-release")?;

    Ok(os_release.release_type.as_str().to_owned())
}

pub(crate) static RELEASE_TYPE: Lazy<String> =
    Lazy::new(|| current_release_type().unwrap());

static HW_VERSION_FILE: OnceLock<String> = OnceLock::new();

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Hardware {
    Diamond,
    Pearl,
}

async fn get_hw_version() -> Result<Hardware> {
    let hw_file = HW_VERSION_FILE.get_or_init(|| {
        env::var("HW_VERSION_FILE")
            .unwrap_or_else(|_| "/usr/persistent/hardware_version".to_string())
    });
    debug!("Reading hardware version from {}", hw_file.as_str());

    let hw = String::from_utf8(
        fs::read(hw_file.as_str())
            .await
            .map_err(|e| {
                tracing::error!(
                    "Executing UI for Pearl as an error occurred while reading file \"{}\": {}",
                    hw_file.as_str(),
                    e
                )
            })
            .unwrap_or_default()
    ).wrap_err("Failed to read HW version")?;

    debug!("Hardware version: {}", hw);

    if hw.contains("Diamond") || hw.contains("B3") {
        Ok(Hardware::Diamond)
    } else {
        Ok(Hardware::Pearl)
    }
}

async fn main_inner(args: Args) -> Result<()> {
    let hw = get_hw_version().await?;
    let serial_device = match hw {
        Hardware::Diamond => Some("/dev/ttyTHS1"),
        Hardware::Pearl => Some("/dev/ttyTHS0"),
    };
    let (mut serial_input_tx, serial_input_rx) = mpsc::channel(INPUT_CAPACITY);

    Serial::spawn(serial_device, serial_input_rx)?;
    match args.subcmd {
        SubCommand::Daemon => {
            if hw == Hardware::Diamond {
                let ui = engine::DiamondJetson::spawn(&mut serial_input_tx);
                let send_ui: &dyn EventChannel = &ui;
                listen(send_ui).await?;
            } else {
                let ui = engine::PearlJetson::spawn(&mut serial_input_tx);
                let send_ui: &dyn EventChannel = &ui;
                listen(send_ui).await?;
            };
        }
        SubCommand::Simulation(args) => {
            let ui: Box<dyn Engine> = if hw == Hardware::Diamond {
                let ui = engine::DiamondJetson::spawn(&mut serial_input_tx);
                ui.clone_tx()
                    .send(Event::Flow {
                        mode: if args == SimulationArgs::Operator {
                            OperatingMode::Operator
                        } else {
                            OperatingMode::SelfServe
                        },
                    })
                    .unwrap();
                Box::new(ui)
            } else {
                let ui = engine::PearlJetson::spawn(&mut serial_input_tx);
                ui.clone_tx()
                    .send(Event::Flow {
                        mode: if args == SimulationArgs::Operator {
                            OperatingMode::Operator
                        } else {
                            OperatingMode::SelfServe
                        },
                    })
                    .unwrap();
                Box::new(ui)
            };
            match args {
                SimulationArgs::SelfServe => {
                    signup_simulation(ui.as_ref(), hw, true, false).await?
                }
                SimulationArgs::Operator => {
                    signup_simulation(ui.as_ref(), hw, false, false).await?
                }
                SimulationArgs::ShowCar => {
                    signup_simulation(ui.as_ref(), hw, true, true).await?
                }
            }
        }
        SubCommand::Beacon(beacon_args) => {
            let ui: Box<dyn Engine> = if hw == Hardware::Diamond {
                let ui = engine::DiamondJetson::spawn(&mut serial_input_tx);
                Box::new(ui)
            } else {
                let ui = engine::PearlJetson::spawn(&mut serial_input_tx);
                Box::new(ui)
            };
            beacon(ui.as_ref(), beacon_args.duration).await?
        }
        SubCommand::Recovery => {
            let ui: Box<dyn Engine> = if hw == Hardware::Diamond {
                Box::new(engine::DiamondJetson::spawn(&mut serial_input_tx))
            } else {
                Box::new(engine::PearlJetson::spawn(&mut serial_input_tx))
            };

            loop {
                ui.recovery();
                time::sleep(Duration::from_secs(45)).await;
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let result = main_inner(args).await;
    telemetry.flush().await;
    result
}

/// Just like `tokio::spawn()`, but if we are using unstable tokio features, we give
/// the task a readable `name`.
fn tokio_spawn<F>(name: &'static str, future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let _name = name; // Deal with "unused" variable;
    #[cfg(tokio_unstable)]
    return tokio::task::Builder::new()
        .name(_name)
        .spawn(future)
        .expect("failed to spawn async task");
    #[cfg(not(tokio_unstable))]
    return tokio::spawn(future);
}
