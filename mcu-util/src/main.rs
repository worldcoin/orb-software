#![forbid(unsafe_code)]
#![allow(clippy::uninlined_format_args)]

use std::path::PathBuf;
use std::time::Duration;

use crate::orb::Orb;
use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::eyre::{Context, Result};
use orb_build_info::{make_build_info, BuildInfo};
use orb_mcu_interface::orb_messages::hardware::OrbVersion;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error};

mod orb;

static BUILD_INFO: BuildInfo = make_build_info!();

/// Utility args
#[derive(Parser, Debug)]
#[clap(
    author,
    version = BUILD_INFO.version,
    about = "Orb MCU utility",
    long_about = "Debug microcontrollers and manage firmware images",
    styles = clap_v3_styles(),
)]
struct Args {
    #[clap(subcommand)]
    subcmd: SubCommand,
    #[clap(short, long, default_value = "false")]
    can_fd: bool,
}

#[derive(Parser, Debug)]
enum SubCommand {
    /// Print Orb's state data
    #[clap(action)]
    Info(InfoOpts),
    /// Reboot a microcontroller. Rebooting the main MCU can be used to reboot the Orb.
    #[clap(subcommand)]
    Reboot(Mcu),
    /// Firmware image handling
    #[clap(subcommand)]
    Image(Image),
    /// Dump one microcontroller messages
    #[clap(action)]
    Dump(DumpOpts),
    /// Stress microcontroller by flooding communication channels
    #[clap(action)]
    Stress(StressOpts),
    /// Control optics: gimbal
    #[clap(subcommand)]
    Optics(OpticsOpts),
    /// Control UI
    #[clap(subcommand)]
    Ui(UiOpts),
    /// Prints hardware revision from main MCU in machine-readable form
    #[clap(action)]
    HardwareRevision {
        ///Path to file to write hardware revision to. If not specified, revision is printed to stdout.
        #[clap(long)]
        filename: Option<PathBuf>,
    },
    #[clap(subcommand)]
    PowerCycle(PowerCycleComponent),
}

#[derive(Parser, Debug)]
pub struct InfoOpts {
    /// Print Orb's diagnostic data
    #[clap(short, long, default_value = "false")]
    diag: bool,
}

#[derive(Parser, Debug)]
pub struct DumpOpts {
    /// Microcontroller
    #[clap(subcommand)]
    mcu: Mcu,
    /// Dump duration in seconds (minimum 10 seconds)
    #[clap(short, long)]
    duration: Option<u64>,
    /// Print only logs from the microcontroller to stdout
    #[clap(short, long, default_value = "false")]
    logs_only: bool,
}

#[derive(Parser, Debug)]
enum Image {
    /// Switch images in slots, with versions checks (prod/dev)
    #[clap(subcommand)]
    Switch(Mcu),
    /// Switch images without safety checks, use if you know what you are doing
    #[clap(subcommand)]
    ForceSwitch(Mcu),
    /// Update microcontroller's firmware
    #[clap(action)]
    Update(McuUpdate),
}

/// Mcu Update options
#[derive(Parser, Debug)]
pub struct McuUpdate {
    /// Mcu
    #[clap(subcommand)]
    mcu: Mcu,
    /// Path to binary file
    #[clap(short, long)]
    path: String,
}

/// Stress tests options
#[derive(Parser, Debug)]
pub struct StressOpts {
    /// Stress test duration in seconds (minimum 10 seconds)
    #[clap(short, long)]
    duration: Option<u64>,
    /// Microcontroller to perform the test on
    #[clap(subcommand)]
    mcu: Mcu,
}

/// Select microcontroller
#[derive(Parser, Debug, Clone, Copy, PartialEq)]
pub enum Mcu {
    /// Main microcontroller
    #[clap(action)]
    Main = 0x01,
    /// Security microcontroller
    #[clap(action)]
    Security = 0x02,
}

/// Optics tests options
#[derive(Parser, Debug, Clone, Copy)]
enum OpticsOpts {
    /// Auto-home the gimbal
    #[clap(subcommand)]
    GimbalHome(GimbalHomeOpts),
    /// Set gimbal position: --phi (millidegree, center is 45000) and --theta (millidegree, center is 90000)
    #[clap(action)]
    GimbalPosition(GimbalPosition),
    /// Move gimbal relative to current position: --phi (right-left) and --theta (up/down)
    #[clap(action)]
    GimbalMove(GimbalPosition),
    /// Test camera trigger for 10 seconds with default options: 30fps, IR-LEDs 100us.
    #[clap(subcommand)]
    TriggerCamera(Camera),
    /// Polarizer command
    #[clap(subcommand)]
    Polarizer(PolarizerOpts),
}

#[derive(Parser, Debug, Clone, Copy)]
enum GimbalHomeOpts {
    /// Auto-home the gimbal by hitting the limits, more accurate but slow and noisy.
    #[clap(action)]
    Autohome,
    /// Shortest path to home the gimbal, doesn't remove any offset.
    #[clap(action)]
    ShortestPath,
}

#[derive(Parser, Debug, Clone, Copy)]
enum PolarizerOpts {
    /// Home the polarizer, takes a few seconds during which the polarizer cannot be used.
    Home,
    /// Set the polarizer to passthrough
    Passthrough,
    /// Set to vertically-polarized
    Vertical,
    /// Set to horizontally-polarized
    Horizontal,
    /// Set custom angle
    Angle {
        /// The angle in decidegrees
        angle: u32,
    },
}

#[derive(Parser, Debug, Clone, Copy)]
enum Camera {
    #[clap(action)]
    Eye {
        /// Frames per second
        #[clap(default_value = "30")]
        fps: u32,
    },
    #[clap(action)]
    Face {
        /// Frames per second
        #[clap(default_value = "30")]
        fps: u32,
    },
}

/// Optics tests options
#[derive(Parser, Debug, Clone, Copy)]
enum UiOpts {
    /// Test front leds for 3 seconds
    #[clap(subcommand)]
    Front(Leds),
}

#[derive(Parser, Debug, Clone, Copy)]
enum Leds {
    #[clap(action)]
    Red,
    #[clap(action)]
    Green,
    #[clap(action)]
    Blue,
    #[clap(action)]
    White,
    #[clap(action)]
    Booster,
}

/// Optics position
#[derive(Parser, Debug, Clone, Copy)]
struct GimbalPosition {
    /// Move mirror right/left. Angle in millidegrees.
    #[clap(short, long, allow_hyphen_values = true)]
    phi: i32,
    /// Move mirror up/down. Angle in millidegrees.
    #[clap(short, long, allow_hyphen_values = true)]
    theta: i32,
}

/// Commands to the secure element
#[derive(Parser, Debug)]
enum PowerCycleComponent {
    /// Power-cycle the secure element
    #[clap(action)]
    SecureElement,
    /// Power-cycle the heat camera
    #[clap(action)]
    HeatCamera,
    /// [dev] Power-cycle the Wifi & BLE module
    #[clap(action)]
    Wifi,
}

async fn execute(args: Args) -> Result<()> {
    let (mut orb, orb_tasks) = Orb::new(args.can_fd).await?;

    match args.subcmd {
        SubCommand::Info(opts) => {
            let orb_info = orb.get_info(opts.diag).await?;
            debug!("{:?}", orb_info);
            println!("{:#}", orb_info);
        }
        SubCommand::Reboot(mcu) => orb.board_mut(mcu).reboot(None).await?,
        SubCommand::Dump(DumpOpts {
            mcu,
            duration,
            logs_only,
        }) => {
            orb.board_mut(mcu)
                .dump(duration.map(Duration::from_secs), logs_only)
                .await?
        }
        SubCommand::Stress(StressOpts { duration, mcu }) => {
            orb.board_mut(mcu)
                .stress_test(duration.map(Duration::from_secs))
                .await?
        }
        SubCommand::Image(Image::Switch(mcu)) => {
            orb.board_mut(mcu).switch_images(false).await?
        }
        SubCommand::Image(Image::ForceSwitch(mcu)) => {
            orb.board_mut(mcu).switch_images(true).await?
        }
        SubCommand::Image(Image::Update(opts)) => {
            orb.board_mut(opts.mcu).update_firmware(&opts.path).await?
        }
        SubCommand::HardwareRevision { filename } => {
            let hw_rev = orb.get_revision().await?;
            // discard operation if unknown hardware version
            if hw_rev.0.version == i32::from(OrbVersion::HwVersionUnknown) {
                return Err(color_eyre::eyre::eyre!(
                    "Failed to fetch hardware revision: unknown"
                ));
            }
            match filename {
                None => {
                    println!("{}", hw_rev);
                }
                Some(ref filename) => {
                    let hw_str = format!("{}", hw_rev);
                    // check that the file exists and compare content with what's going to be
                    // written to avoid writing the same content.
                    if let Ok(existing_content) = fs::read_to_string(filename)
                        .await
                        .with_context(|| format!("Failed to read file {filename:?}"))
                    {
                        if existing_content == hw_str {
                            debug!("Content is the same, skipping write");
                            return Ok(());
                        }
                    }

                    // overwrite the file with the new content
                    let mut file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .truncate(true)
                        .create(true)
                        .open(filename)
                        .await
                        .with_context(|| format!("Failed to open file {filename:?}"))?;
                    file.set_len(0).await?;
                    if file.write(hw_str.as_bytes()).await? != hw_str.len() {
                        return Err(color_eyre::eyre::eyre!("Failed to write to file"));
                    }
                    file.flush().await?;
                }
            }
        }
        SubCommand::Optics(opts) => match opts {
            OpticsOpts::GimbalHome(opts) => {
                orb.main_board_mut().gimbal_auto_home(opts).await?
            }
            OpticsOpts::GimbalPosition(opts) => {
                if opts.phi < 0 || opts.theta < 0 {
                    return Err(color_eyre::eyre::eyre!("Angles must be positive"));
                }
                orb.main_board_mut()
                    .gimbal_set_position(opts.phi as u32, opts.theta as u32)
                    .await?
            }
            OpticsOpts::GimbalMove(opts) => {
                orb.main_board_mut()
                    .gimbal_move(opts.phi, opts.theta)
                    .await?
            }
            OpticsOpts::TriggerCamera(camera) => {
                let fps = match camera {
                    Camera::Eye { fps } => fps,
                    Camera::Face { fps } => fps,
                };
                orb.main_board_mut().trigger_camera(camera, fps).await?
            }
            OpticsOpts::Polarizer(opts) => orb.main_board_mut().polarizer(opts).await?,
        },
        SubCommand::PowerCycle(opts) => match opts {
            PowerCycleComponent::SecureElement => {
                orb.sec_board_mut().power_cycle_secure_element().await?
            }
            PowerCycleComponent::HeatCamera => {
                orb.main_board_mut().heat_camera_power_cycle().await?
            }
            PowerCycleComponent::Wifi => {
                orb.main_board_mut().wifi_power_cycle().await?
            }
        },
        SubCommand::Ui(opts) => match opts {
            UiOpts::Front(leds) => orb.main_board_mut().front_leds(leds).await?,
        },
    }

    // Kills the tasks
    drop(orb);
    // Timeout because tasks might never check the kill signal because they are busy
    // waiting to receive another message. In the event we timeout, most likely there
    // have been no errors.
    // TODO: We need to make the synchronous actually use nonblocking code to make the
    // timeout unecessary
    match tokio::time::timeout(Duration::from_millis(100), orb_tasks.join()).await {
        Ok(result) => result,
        Err(tokio::time::error::Elapsed { .. }) => Ok(()),
    }
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new().init();

    let args = Args::parse();

    if cfg!(debug_assertions) {
        debug!("{:?}", args);
    }

    if let Err(e) = execute(args).await {
        error!("{:#?}", e);
        telemetry.flush().await;
        std::process::exit(-1);
    } else {
        telemetry.flush().await;
        std::process::exit(0);
    }
}
