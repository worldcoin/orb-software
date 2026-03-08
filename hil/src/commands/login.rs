use std::{num::NonZeroU8, path::PathBuf, time::Duration};

use bytes::Bytes;
use clap::Parser;
use color_eyre::{
    eyre::{bail, ContextCompat, WrapErr},
    Result,
};
use futures::FutureExt as _;
use humantime::parse_duration;
use secrecy::{ExposeSecret as _, SecretString};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt as _},
    sync::broadcast::{self},
};
use tokio_serial::SerialPortBuilderExt as _;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{info, warn};

use crate::orb::OrbConfig;
use crate::serial::{spawn_serial_reader_task, wait_for_pattern};

const LOGIN_PROMPT_USER: &str = "worldcoin";

#[derive(Debug, Parser)]
pub struct Login {
    #[command(flatten)]
    orb_config: OrbConfig,
    #[arg(long)]
    password: SecretString,
    /// Timeout duration per-attempt (e.g., "10s", "500ms")
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    timeout: Duration,
    #[arg(long, default_value = "3")]
    max_attempts: NonZeroU8,
}

impl Login {
    /// Get the serial port path from orb_config
    fn get_serial_path(orb_config: &OrbConfig) -> Result<&PathBuf> {
        orb_config
            .serial_path
            .as_ref()
            .wrap_err("serial-path must be specified")
    }

    pub async fn run(self) -> Result<()> {
        let orb_config = self.orb_config.use_file_if_exists()?;
        let serial_path = Login::get_serial_path(&orb_config)?;

        let serial = tokio_serial::new(
            serial_path.to_string_lossy(),
            crate::serial::ORB_BAUD_RATE,
        )
        .open_native_async()
        .wrap_err_with(|| {
            format!("failed to open serial port {}", serial_path.display())
        })?;

        let (serial_reader, mut serial_writer) = tokio::io::split(serial);
        let (serial_output_tx, mut serial_output_rx) = broadcast::channel(64);
        let (reader_task, kill_tx) =
            spawn_serial_reader_task(serial_reader, serial_output_tx);

        let login_fut = async move {
            let mut attempts_remaining = self.max_attempts.get();
            let result = loop {
                let inner_result = Self::do_login(
                    &mut serial_writer,
                    &mut serial_output_rx,
                    &self.password,
                    self.timeout,
                )
                .await
                .wrap_err("failed to perform login procedure");
                attempts_remaining -= 1;
                if inner_result.is_ok() || attempts_remaining == 0 {
                    break inner_result;
                }
                warn!(
                    "failed to perform login procedure, retrying...: {inner_result:?}"
                );
            };
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
    /// Times out if prompt cannot be detected within timeout.
    async fn do_login(
        mut serial_writer: impl AsyncWrite + Unpin,
        serial_rx: &mut broadcast::Receiver<Bytes>,
        password: &SecretString,
        timeout: Duration,
    ) -> Result<()> {
        // exit prompt in case this is a retry
        serial_writer
            .write_all("\x04".as_bytes())
            .await
            .wrap_err("error writing ctrl-d")?;

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
        // overall timeout, in case prompt is not found
        let timeout_fut = tokio::time::sleep(timeout);

        let () = tokio::select! {
            _ = timeout_fut => bail!("failed to detect login prompt"),
            result = type_enter_fut => return Err(result.expect_err("ok variant unreachable")),
            result = wait_fut => result?, // continues rest of function if Ok, if happy path.
        };
        info!("Detected login prompt!");
        tokio::time::sleep(Duration::from_millis(200)).await;

        info!("Entering username");
        serial_writer
            .write_all(format!("{LOGIN_PROMPT_USER}\n").as_bytes())
            .await
            .wrap_err("error while typing username")?;
        tokio::time::sleep(Duration::from_millis(2000)).await;

        info!("Entering password");
        let serial_rx_copy = BroadcastStream::new(serial_rx.resubscribe());
        serial_writer
            .write_all(format!("{}\n", password.expose_secret()).as_bytes())
            .await
            .wrap_err("error while typing username")?;
        tokio::time::timeout(
            Duration::from_millis(45000),
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
