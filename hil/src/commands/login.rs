use std::{path::PathBuf, time::Duration};

use bytes::Bytes;
use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use futures::FutureExt as _;
use secrecy::{ExposeSecret as _, SecretString};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt as _},
    sync::broadcast::{self},
};
use tokio_serial::SerialPortBuilderExt as _;
use tokio_stream::wrappers::BroadcastStream;
use tracing::info;

use crate::serial::{spawn_serial_reader_task, wait_for_pattern};

const LOGIN_PROMPT_TIMEOUT: Duration = Duration::from_secs(60);
const LOGIN_PROMPT_USER: &str = "worldcoin";

#[derive(Debug, Parser)]
pub struct Login {
    #[arg(long, default_value = crate::serial::DEFAULT_SERIAL_PATH)]
    serial_path: PathBuf,
    #[arg(long)]
    password: SecretString,
}

impl Login {
    pub async fn run(self) -> Result<()> {
        let serial = tokio_serial::new(
            self.serial_path.to_string_lossy(),
            crate::serial::ORB_BAUD_RATE,
        )
        .open_native_async()
        .wrap_err_with(|| {
            format!("failed to open serial port {}", self.serial_path.display())
        })?;

        let (serial_reader, serial_writer) = tokio::io::split(serial);
        let (serial_output_tx, serial_output_rx) = broadcast::channel(64);
        let (reader_task, kill_tx) =
            spawn_serial_reader_task(serial_reader, serial_output_tx);

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
        serial_rx: broadcast::Receiver<Bytes>,
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
