#![forbid(unsafe_code)]

mod boot;
mod download_s3;
mod flash;
mod ftdi;
mod serial;

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result,
};
use flash::FlashVariant;
use orb_build_info::{make_build_info, BuildInfo};
use tokio_serial::SerialPortBuilderExt as _;
use tokio_stream::StreamExt as _;
use tracing::{debug, info, warn};
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};

use std::path::PathBuf;

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser, Debug)]
#[command(about, author, version=BUILD_INFO.version, styles=make_clap_v3_styles())]
struct Cli {
    #[command(subcommand)]
    commands: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Flash(Flash),
    Reboot(Reboot),
    Login(Login),
}

#[derive(Parser, Debug)]
struct Flash {
    /// The s3 URI of the rts.
    #[arg(long)]
    s3_url: Option<String>,
    /// The directory to save the s3 artifact we download.
    #[arg(long)]
    download_dir: Option<Utf8PathBuf>,
    /// Skips download by using an existing tarball on the filesystem.
    #[arg(long)]
    rts_path: Option<Utf8PathBuf>,
    /// if this flag is given, uses flashcmd.txt instead of fastflashcmd.txt
    #[arg(long)]
    slow: bool,
}

impl Flash {
    async fn run(self) -> Result<()> {
        let args = self;
        ensure!(
            crate::boot::is_recovery_mode_detected()?,
            "orb must be in recovery mode to flash. Try running `orb-hil reboot -r`"
        );
        let rts_path = if let Some(ref s3_url) = args.s3_url {
            if args.rts_path.is_some() {
                bail!("both rts_path and s3_url were specified - only provide one or the other");
            }
            let download_dir = args.download_dir.unwrap_or(current_dir());
            let download_path = download_dir.join(
                crate::download_s3::parse_filename(s3_url)
                    .wrap_err("failed to parse filename")?,
            );

            crate::download_s3::download_url(s3_url, &download_path)
                .await
                .wrap_err("error while downloading from s3")?;

            download_path
        } else if let Some(rts_path) = args.rts_path {
            if args.s3_url.is_some() {
                bail!("both rts-path and s3-url were specified - only provide one or the other");
            }
            if args.download_dir.is_some() {
                bail!("both rts-path and download-dir were specified - only provide one or the other");
            }
            info!("using already downloaded rts tarball");
            rts_path
        } else {
            bail!("you must provide either rts-path or s3-url");
        };

        let variant = if args.slow {
            FlashVariant::Regular
        } else {
            FlashVariant::Fast
        };
        crate::flash::flash(variant, &rts_path)
            .await
            .wrap_err("error while flashing")?;

        Ok(())
    }
}

#[derive(Debug, Parser)]
struct Reboot {
    #[arg(short)]
    recovery: bool,
}

impl Reboot {
    async fn run(self) -> Result<()> {
        crate::boot::reboot(self.recovery).await.wrap_err_with(|| {
            format!(
                "failed to reboot into {} mode",
                if self.recovery { "recovery" } else { "normal" }
            )
        })
    }
}

#[derive(Debug, Parser)]
struct Login {
    #[arg(long, default_value = crate::serial::DEFAULT_SERIAL_PATH)]
    serial_path: PathBuf,
    #[arg(long)]
    password: String,
}

impl Login {
    async fn run(self) -> Result<()> {
        let serial = tokio_serial::new(
            self.serial_path.to_string_lossy(),
            crate::serial::ORB_BAUD_RATE,
        )
        .open_native_async()
        .wrap_err_with(|| {
            format!("failed to open serial port {}", self.serial_path.display())
        })?;

        let (serial_tx, serial_rx) = tokio::sync::broadcast::channel(64);
        let _serial_join_handle = tokio::task::spawn(async move {
            let mut serial_stream = tokio_util::io::ReaderStream::new(serial);
            loop {
                let Some(chunk) = serial_stream
                    .try_next()
                    .await
                    .wrap_err("failed to read from serial")?
                else {
                    break;
                };
                if let Err(_err) = serial_tx.send(chunk) {
                    warn!(
                        "dropping serial data due to slow receivers. \
                        Consider a larger channel size"
                    );
                }
            }
            debug!("terminating serial task due to end of stream");
            Ok::<(), color_eyre::Report>(())
        });
        let serial_rx = tokio_stream::wrappers::BroadcastStream::new(serial_rx);
        crate::serial::wait_for_login_prompt(serial_rx)
            .await
            .wrap_err("failed to wait for login prompt")
    }
}

fn current_dir() -> Utf8PathBuf {
    std::env::current_dir().unwrap().try_into().unwrap()
}

fn make_clap_v3_styles() -> clap::builder::Styles {
    use clap::builder::styling::AnsiColor;
    clap::builder::Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let args = Cli::parse();
    let run_fut = async {
        match args.commands {
            Commands::Flash(c) => c.run().await,
            Commands::Login(c) => c.run().await,
            Commands::Reboot(c) => c.run().await,
        }
    };
    tokio::select! {
        result = run_fut => result,
        // Needed to cleanly call destructors.
        result = tokio::signal::ctrl_c() => result.wrap_err("failed to listen for ctrl-c"),
    }
}
