#![forbid(unsafe_code)]

mod boot;
mod download_s3;
mod flash;
mod ftdi;
mod serial;

use bytes::Bytes;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result,
};
use flash::FlashVariant;
use futures::FutureExt as _;
use orb_build_info::{make_build_info, BuildInfo};
use secrecy::{ExposeSecret as _, SecretString};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt as _},
    sync::broadcast::error::SendError,
};
use tokio_serial::SerialPortBuilderExt as _;
use tokio_stream::{wrappers::BroadcastStream, StreamExt as _};
use tracing::{debug, info};
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};

use std::{path::PathBuf, time::Duration};

use crate::serial::wait_for_pattern;

const BUILD_INFO: BuildInfo = make_build_info!();
const LOGIN_PROMPT_TIMEOUT: Duration = Duration::from_secs(60);
const LOGIN_PROMPT_USER: &str = "worldcoin";

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
    password: SecretString,
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

        let (serial_writer, serial_output_rx, reader_task, kill_tx) = {
            let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel();
            let (reader, writer) = tokio::io::split(serial);
            let (serial_output_tx, serial_output_rx) =
                tokio::sync::broadcast::channel(64);
            let reader_task = tokio::task::spawn(async move {
                let mut serial_stream = tokio_util::io::ReaderStream::new(reader);
                let mut stderr = tokio::io::stderr();
                loop {
                    let chunk = tokio::select! {
                        _ = &mut kill_rx => break,
                        chunk = serial_stream.try_next() => chunk,
                    };
                    let Some(chunk) = chunk.wrap_err("failed to read from serial")?
                    else {
                        break;
                    };
                    let _ = stderr.write_all(&chunk).await;
                    if let Err(SendError(_)) = serial_output_tx.send(chunk) {
                        break;
                    }
                }
                debug!("terminating serial task");
                Ok::<(), color_eyre::Report>(())
            });

            (writer, serial_output_rx, reader_task, kill_tx)
        };

        let login_fut = async {
            let result = Self::do_login(serial_writer, serial_output_rx, self.password)
                .await
                .wrap_err("failed to perform login procedure");
            let _ = kill_tx.send(());
            result
        };
        let ((), ()) = tokio::try_join! {
            login_fut,
            reader_task.map(|r| r.wrap_err("serial reader task panicked")?),
        }?;

        Ok(())
    }

    /// Waits for login prompt, while typing enter key. Then when detected, enters
    /// password.
    ///
    /// Times out if prompt cannot be detected within [`LOGIN_PROMPT_TIMEOUT`].
    async fn do_login(
        mut serial_writer: impl AsyncWrite + Unpin,
        serial_rx: tokio::sync::broadcast::Receiver<Bytes>,
        password: SecretString,
    ) -> Result<()> {
        let wait_fut = crate::serial::wait_for_pattern(
            crate::serial::LOGIN_PROMPT_PATTERN.to_owned().into_bytes(),
            tokio_stream::wrappers::BroadcastStream::new(serial_rx.resubscribe()),
        )
        .map(|r| r.wrap_err("failed to wait for login prompt"));
        // types the enter key repeatedly to trigger prompt
        let type_enter_fut = async {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                serial_writer
                    .write_all("\n".as_bytes())
                    .await
                    .wrap_err("error writing newline")?;
            }
        }
        .map(|r: Result<()>| r.wrap_err("error while typing enter key"));
        // overall timeout, incase prompt is not found
        let timeout_fut = tokio::time::sleep(LOGIN_PROMPT_TIMEOUT);

        let () = tokio::select! {
            _ = timeout_fut => bail!("failed to detect login prompt"),
            result = type_enter_fut => return Err(result.expect_err("ok variant unreachable")),
            result = wait_fut => result?, // continues rest of function if Ok, if happy path.
        };
        info!("Detected login prompt!");

        info!("Entering username");
        serial_writer
            .write_all(format!("{LOGIN_PROMPT_USER}\n").as_bytes())
            .await
            .wrap_err("error while typing username")?;
        tokio::time::sleep(Duration::from_millis(200)).await;

        info!("Entering password");
        let serial_rx_copy = BroadcastStream::new(serial_rx.resubscribe());
        serial_writer
            .write_all(format!("{}\n", password.expose_secret()).as_bytes())
            .await
            .wrap_err("error while typing username")?;
        tokio::time::timeout(
            Duration::from_millis(5000),
            wait_for_pattern("worldcoin@id".as_bytes().to_owned(), serial_rx_copy),
        )
        .await
        .wrap_err("timeout while waiting for bash prompt")?
        .wrap_err("error while waiting for bash prompt")?;

        // Double check that the login was successful, by running `whoami`.
        info!("Running whoami");
        let serial_rx_copy = BroadcastStream::new(serial_rx.resubscribe());
        serial_writer
            .write_all("whoami\n".as_bytes())
            .await
            .wrap_err("failed to type after logging in")?;
        tokio::time::timeout(
            Duration::from_millis(5000),
            wait_for_pattern(LOGIN_PROMPT_USER.to_owned().into_bytes(), serial_rx_copy),
        )
        .await
        .wrap_err("whoami response timed out")?
        .wrap_err("error while listening for whoami response")?;
        info!("whoami response detected! We are good to go");

        Ok(())
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
