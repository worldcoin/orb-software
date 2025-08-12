#![allow(clippy::uninlined_format_args)]
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use cmd_lib::run_cmd;
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result, Section,
};
use orb_s3_helpers::ExistingFileBehavior;
use tempfile::TempDir;
use tracing::info;

use crate::{boot::is_recovery_mode_detected, current_dir};

#[derive(Parser, Debug)]
#[command(about = "Fetch persistent partitions from the orb device")]
pub struct FetchPersistent {
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
    /// If this flag is given, overwites any existing files when downloading the rts.
    #[arg(long)]
    overwrite_existing: bool,
    /// Path to save the fetched persistent partition files
    #[arg(long)]
    save_persistent_path: Option<Utf8PathBuf>,
}

impl FetchPersistent {
    pub async fn run(self) -> Result<()> {
        let args = self;
        let existing_file_behavior = if args.overwrite_existing {
            ExistingFileBehavior::Overwrite
        } else {
            ExistingFileBehavior::Abort
        };
        ensure!(
            is_recovery_mode_detected()?,
            "orb must be in recovery mode to fetch persistent partitions. Try running `orb-hil reboot -r`"
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

        let save_persistent_path = if let Some(save_path) = args.save_persistent_path {
            save_path
        } else {
            bail!("--save-persistent-path is required");
        };

        // Validate output directory exists
        ensure!(
            save_persistent_path
                .parent()
                .unwrap()
                .try_exists()
                .unwrap_or(false),
            "Output directory {:?} doesn't exist",
            save_persistent_path.parent().unwrap()
        );

        fetch_persistent(&rts_path, &save_persistent_path)
            .await
            .wrap_err("error while fetching persistent partitions")?;

        Ok(())
    }
}

pub async fn fetch_persistent(
    path_to_rts_tar: &Utf8Path,
    save_persistent_path: &Utf8PathBuf,
) -> Result<()> {
    let path_to_rts = path_to_rts_tar.to_owned();
    let save_persistent_path = save_persistent_path.to_owned();
    tokio::task::spawn_blocking(move || {
        ensure!(is_recovery_mode_detected()?, "orb not in recovery mode");
        let tmp_dir = extract(&path_to_rts)?;
        println!("Extracted RTS to: {tmp_dir:?}");
        ensure!(is_recovery_mode_detected()?, "orb not in recovery mode");
        fetch_persistent_cmd(tmp_dir.path(), &save_persistent_path)?;
        Ok(())
    })
    .await
    .wrap_err("task panicked")?
}

fn extract(path_to_rts: &Utf8Path) -> Result<TempDir> {
    ensure!(
        path_to_rts.try_exists().unwrap_or(false),
        "{path_to_rts} doesn't exist"
    );
    ensure!(path_to_rts.is_file(), "{path_to_rts} should be a file!");
    let path_to_rts = path_to_rts
        .canonicalize()
        .wrap_err_with(|| format!("failed to canonicalize path: {}", path_to_rts))?;
    let temp_dir = TempDir::new_in(path_to_rts.parent().unwrap())
        .wrap_err("failed to create temporary extract dir")?;
    let extract_dir = temp_dir.path();

    let result = run_cmd! {
        cd $extract_dir;
        info extracting rts $path_to_rts;
        tar xvf $path_to_rts;
        info finished extract!;
    };
    result
        .wrap_err("failed to extract rts")
        .with_note(|| format!("path_to_rts was {}", path_to_rts.display()))?;

    Ok(temp_dir)
}

fn fetch_persistent_cmd(
    extracted_dir: &Path,
    save_persistent_path: &Utf8Path,
) -> Result<()> {
    let bootloader_dir = extracted_dir.join("ready-to-sign").join("bootloader");
    ensure!(
        bootloader_dir.try_exists().unwrap_or(false),
        "{bootloader_dir:?} doesn't exist"
    );

    let result = run_cmd! {
        cd $bootloader_dir;
        info "Running tegraflash to fetch persistent partitions";
        bash hil-fetch-persistentcmd.txt;
        info "Finished fetching persistent partitions!";
    };
    result
        .wrap_err("failed to fetch persistent partitions")
        .with_note(|| format!("bootloader_dir was {bootloader_dir:?}"))?;

    // Copy the fetched files to the output path
    let copy_result = run_cmd! {
        info "Copying fetched partition files to output path";
        cp $bootloader_dir/persistent.img $save_persistent_path/persistent.img;
        cp $bootloader_dir/persistent-journaled.img $save_persistent_path/persistent-journaled.img;
        cp $bootloader_dir/uid $save_persistent_path/uid;
        cp $bootloader_dir/uid.pub $save_persistent_path/uid.pub;
        info "Files copied to {}", $save_persistent_path;
    };
    copy_result
        .wrap_err("failed to copy fetched files to output path")
        .with_note(|| format!("save_persistent_path was {save_persistent_path}"))?;

    Ok(())
}
