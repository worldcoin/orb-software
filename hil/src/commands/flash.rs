use camino::Utf8PathBuf;
use clap::Parser;
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result,
};
use tracing::info;

use crate::{current_dir, download_s3::ExistingFileBehavior, flash::FlashVariant};

#[derive(Parser, Debug)]
pub struct Flash {
    /// The s3 URI of the rts.
    #[arg(
        long,
        conflicts_with = "rts_path",
        required_unless_present = "rts_path"
    )]
    s3_url: Option<String>,
    /// The directory to save the s3 artifact we download.
    #[arg(long)]
    download_dir: Option<Utf8PathBuf>,
    /// Skips download by using an existing tarball on the filesystem.
    #[arg(long, conflicts_with = "s3_url", required_unless_present = "s3_url")]
    rts_path: Option<Utf8PathBuf>,
    /// If this flag is given, uses flashcmd.txt instead of fastflashcmd.txt
    #[arg(long)]
    slow: bool,
    /// If this flag is given, overwites any existing files when downloading the rts.
    #[arg(long)]
    overwrite_existing: bool,
}

impl Flash {
    pub async fn run(self) -> Result<()> {
        let args = self;
        let existing_file_behavior = if args.overwrite_existing {
            ExistingFileBehavior::Overwrite
        } else {
            ExistingFileBehavior::Abort
        };
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
