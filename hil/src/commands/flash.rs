use camino::Utf8PathBuf;
use chrono::{DateTime, SecondsFormat, Utc};
use clap::Parser;
use color_eyre::{
    eyre::{bail, ensure, ContextCompat, WrapErr},
    Result,
};
use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio_serial::SerialPortBuilderExt as _;
use tracing::{info, warn};

use crate::OrbConfig;
use orb_s3_helpers::{ExistingFileBehavior, S3Uri};
use rand::{rngs::StdRng, SeedableRng};
use serde_json::json;

use crate::{current_dir, rts::FlashVariant};

#[derive(Parser, Debug)]
pub struct Flash {
    /// The s3 URI of the rts.
    #[arg(
        long,
        conflicts_with = "rts_path",
        required_unless_present = "rts_path"
    )]
    s3_url: Option<S3Uri>,
    /// The directory to save the s3 artifact we download.
    #[arg(long)]
    download_dir: Option<Utf8PathBuf>,
    /// Path to a downloaded RTS (zipped .tar or an already-extracted directory).
    #[arg(long, conflicts_with = "s3_url", required_unless_present = "s3_url")]
    rts_path: Option<Utf8PathBuf>,
    /// If this flag is given, uses fastflashcmd.txt instead of flashcmd.txt
    #[arg(long)]
    fast: bool,
    /// If this flag is given, uses hil-flashcmd.txt instead of flashcmd.txt
    #[arg(long)]
    ci: bool,
    /// If this flag is given, overwites any existing files when downloading the rts.
    #[arg(long)]
    overwrite_existing: bool,
    /// Path to directory containing persistent .img files to copy to bootloader dir.
    /// Defaults to /home/$USER/persistent-$ORB_ID
    #[arg(long)]
    persistent_img_path: Option<Utf8PathBuf>,
}

impl Flash {
    fn get_serial_path(orb_config: &OrbConfig) -> Result<&PathBuf> {
        orb_config
            .serial_path
            .as_ref()
            .wrap_err("serial-path must be specified")
    }

    pub async fn run(self, orb_config: &OrbConfig) -> Result<()> {
        let args = self;
        let existing_file_behavior = if args.overwrite_existing {
            ExistingFileBehavior::Overwrite
        } else {
            ExistingFileBehavior::Abort
        };
        ensure!(
            crate::boot::is_recovery_mode_detected().await?,
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

            crate::download_s3::download_url(
                s3_url,
                &download_path,
                existing_file_behavior,
            )
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

        let variant = match (args.fast, args.ci) {
            (true, true) => FlashVariant::HilFast,
            (true, false) => FlashVariant::Fast,
            (false, true) => FlashVariant::Hil,
            (false, false) => FlashVariant::Regular,
        };
        let persistent_img_path = args.persistent_img_path.or_else(|| {
            let home = env::var("HOME").ok()?;
            let orb_id = orb_config.orb_id.as_deref()?;
            Some(Utf8PathBuf::from(format!("{home}/persistent-{orb_id}")))
        });
        let flash_started = Instant::now();
        crate::rts::flash(
            variant,
            &rts_path,
            persistent_img_path.as_deref().map(|p| p.as_std_path()),
            StdRng::from_rng(rand::thread_rng()).unwrap(),
        )
        .await
        .wrap_err("error while flashing")?;
        let flash_duration_seconds = flash_started.elapsed().as_secs();
        let flash_completed_at = Utc::now();

        if args.ci {
            Self::emit_ci_metrics(
                orb_config,
                flash_duration_seconds,
                flash_completed_at,
            )
            .await;
        }

        Ok(())
    }

    async fn emit_ci_metrics(
        orb_config: &OrbConfig,
        flash_duration_seconds: u64,
        flash_completed_at: DateTime<Utc>,
    ) {
        let login_prompt_detected_at =
            match Self::wait_for_login_prompt(orb_config, Duration::from_secs(15 * 60))
                .await
            {
                Ok(timestamp) => Some(timestamp),
                Err(error) => {
                    warn!("failed to wait for login prompt after flash: {error:?}");
                    None
                }
            };

        let boot_to_login_prompt_duration_seconds =
            login_prompt_detected_at.and_then(|timestamp| {
                (timestamp - flash_completed_at)
                    .to_std()
                    .ok()
                    .map(|d| d.as_secs())
            });

        let metrics = json!({
            "event": "hil_flash_metrics",
            "flash_duration_seconds": flash_duration_seconds,
            "flash_completed_at": flash_completed_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            "boot_to_login_prompt_duration_seconds": boot_to_login_prompt_duration_seconds,
            "login_prompt_detected_at": login_prompt_detected_at.map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Secs, true)),
        });

        println!("{metrics}");
    }

    async fn wait_for_login_prompt(
        orb_config: &OrbConfig,
        timeout: Duration,
    ) -> Result<DateTime<Utc>> {
        let serial_path = Self::get_serial_path(orb_config)?;
        let serial = tokio_serial::new(
            serial_path.to_string_lossy(),
            crate::serial::ORB_BAUD_RATE,
        )
        .open_native_async()
        .wrap_err_with(|| {
            format!("failed to open serial port {}", serial_path.display())
        })?;

        let (serial_reader, _) = tokio::io::split(serial);
        let (serial_output_tx, serial_output_rx) = tokio::sync::broadcast::channel(64);
        let (reader_task, kill_tx) =
            crate::serial::spawn_serial_reader_task(serial_reader, serial_output_tx);

        info!("waiting for login prompt after flash");
        let login_wait_result = tokio::time::timeout(
            timeout,
            crate::serial::wait_for_pattern(
                crate::serial::LOGIN_PROMPT_PATTERN.as_bytes().to_vec(),
                tokio_stream::wrappers::BroadcastStream::new(serial_output_rx),
            ),
        )
        .await
        .wrap_err("timed out while waiting for login prompt after flash")?
        .wrap_err("failed while waiting for login prompt after flash");

        let _ = kill_tx.send(());
        reader_task
            .await
            .wrap_err("serial reader task panicked")??;

        login_wait_result?;
        Ok(Utc::now())
    }
}
