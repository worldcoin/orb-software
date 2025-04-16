use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use crate::{diff_ota::diff_ota, file_or_stdout::stdout_if_none, ota_path::OtaPath};
use async_tempfile::TempDir;
use bidiff::DiffParams;
use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use clap_stdin::FileOrStdin;
use color_eyre::{
    eyre::{ensure, WrapErr as _},
    Result,
};
use orb_build_info::{make_build_info, BuildInfo};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub mod diff_ota;
pub mod fetch;
pub mod file_or_stdout;
pub mod ota_path;

const BUILD_INFO: BuildInfo = make_build_info!();

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

/// Commands for binary diffing
#[derive(Debug, Parser)]
#[clap(
    author,
    about,
    styles = clap_v3_styles(),
    version=BUILD_INFO.version,
)]
pub struct Args {
    #[command(subcommand)]
    subcommand: Subcommands,
}
impl Args {
    pub async fn run(self, cancel: CancellationToken) -> Result<()> {
        self.subcommand.run(cancel).await
    }
}

#[derive(Debug, Parser)]
enum Subcommands {
    Diff(DiffCmd),
    Patch(PatchCmd),
    Ota(OtaCmd),
}

impl Subcommands {
    pub async fn run(self, cancel: CancellationToken) -> Result<()> {
        match self {
            Subcommands::Diff(c) => tokio::task::spawn_blocking(|| run_diff_cmd(c))
                .await
                .wrap_err("task panicked")
                .and_then(|r| r),
            Subcommands::Patch(c) => tokio::task::spawn_blocking(|| run_patch_cmd(c))
                .await
                .wrap_err("task panicked")
                .and_then(|r| r),
            Subcommands::Ota(c) => {
                cancel
                    .run_until_cancelled(run_ota_cmd(c, cancel.clone()))
                    .await
                    .unwrap_or(Ok(())) // unwrap if cancelled
            }
        }
    }
}

/// Bidiff two files.
#[derive(Debug, Parser)]
pub struct DiffCmd {
    /// The "base" file, aka the initial state.
    #[clap(long)]
    base: PathBuf,
    /// The "top" file, aka the final state.
    #[clap(long)]
    top: PathBuf,
    /// The location of the new file to output to. If not provided and a tty, outputs
    /// to stdout.
    #[clap(long, short)]
    out: Option<PathBuf>,
}

/// Apply a bidiff patch
#[derive(Debug, Parser)]
pub struct PatchCmd {
    /// The "base" file, aka the initial state.
    #[clap(long)]
    base: PathBuf,
    /// The "patch" file, which contains the diff contents.
    #[clap(long)]
    patch: FileOrStdin,
    /// The location of the new file to output to. If not provided and a tty, outputs
    /// to stdout
    #[clap(long, short)]
    out: Option<PathBuf>,
    #[clap(long, short)]
    force_overwrite_file: bool,
}

/// Bidiff an entire ota.
#[derive(Debug, Parser)]
pub struct OtaCmd {
    /// The "base" ota, i.e. the state before transition.
    /// Supports either `s3://...`, `ota://X.Y.Z...`, or a path.
    #[clap(long, short)]
    base: OtaPath,
    /// The "top" ota, i.e. the state after transition.
    /// Supports either `s3://...`, `ota://X.Y.Z...`, or a path.
    #[clap(long, short)]
    top: OtaPath,
    /// The directory to output the finished OTA. Must be empty if it exists.
    #[clap(long, short)]
    out: PathBuf,
    /// The location that any downloaded OTAs will be placed. If `None`, they will
    /// go to a temporary directory in the current working dir.
    #[clap(long, short)]
    download_dir: Option<PathBuf>,
    #[clap(long)]
    skip_input_hashing: bool,
}

pub async fn is_empty_dir(d: &Path) -> Result<bool> {
    Ok(tokio::fs::read_dir(d).await?.next_entry().await?.is_none())
}

fn progress_bar_style() -> indicatif::ProgressStyle {
    indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] ({msg}) [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("#>-")
}

fn run_diff_cmd(args: DiffCmd) -> Result<()> {
    // TODO: instead of reading the entire file, it may make sense to memmap large files
    let base_contents = fs::read(&args.base).wrap_err("failed to read base file")?;
    let top_contents = fs::read(&args.top).wrap_err("failed to read top file")?;
    let out_writer = io::BufWriter::new(stdout_if_none(args.out.as_deref(), false)?);
    let mut zstd_encoder =
        zstd::Encoder::new(out_writer, 0).expect("infallible: 0 should always work");

    orb_bidiff_squashfs::diff_squashfs()
        .old_path(&args.base)
        .old(&base_contents)
        .new_path(&args.top)
        .new(&top_contents)
        .out(&mut zstd_encoder)
        .diff_params(&DiffParams::default())
        .call()
        .wrap_err("failed to perform diff")?;

    zstd_encoder
        .finish()
        .and_then(|buf_writer| buf_writer.into_inner().map_err(|e| e.into_error()))
        .and_then(|mut file| file.flush())
        .wrap_err("failed to flush writer")
}

fn run_patch_cmd(args: PatchCmd) -> Result<()> {
    // TODO: Check that base is a zstd squashfs. Bipatch will work with any type of file
    // but its better to be overly precise on how to use this tool.
    let base_reader = io::BufReader::new(
        std::fs::File::open(args.base).wrap_err("failed to read base file")?,
    );
    let patch_reader = io::BufReader::new(
        args.patch
            .into_reader()
            .wrap_err("failed to read patch file")?,
    );

    let zstd_reader = zstd::Decoder::new(patch_reader)
        .wrap_err("failed to create zstd decoder from patch file")?;
    let mut out_writer = io::BufWriter::new(
        stdout_if_none(args.out.as_deref(), args.force_overwrite_file)
            .wrap_err("failed to open out file")?,
    );

    let mut patch_processor = bipatch::Reader::new(zstd_reader, base_reader)
        .wrap_err("failed to decode patch")?;
    let nbytes = std::io::copy(&mut patch_processor, &mut out_writer)
        .wrap_err("failed to apply patch")?;
    info!("wrote {nbytes} bytes");
    out_writer
        .into_inner()
        .wrap_err("failed to flush bufwriter")?
        .flush()
        .wrap_err("failed to flush writer")?;

    Ok(())
}

async fn run_ota_cmd(args: OtaCmd, cancel: CancellationToken) -> Result<()> {
    let _cancel_guard = cancel.clone().drop_guard();

    if tokio::fs::try_exists(&args.out).await? {
        let is_empty = is_empty_dir(&args.out).await.wrap_err_with(|| {
            format!("out dir {} cannot be read", args.out.display())
        })?;
        ensure!(is_empty, "out dir {} must be empty", args.out.display());
    } else {
        tokio::fs::create_dir_all(&args.out)
            .await
            .wrap_err_with(|| {
                format!("failed to create out dir `{}`", args.out.display())
            })?;
    }

    let tmpdir = if args.download_dir.is_none() {
        Some(
            TempDir::new_in(Path::new("."))
                .await
                .expect("should be able to create tempdir in current dir"),
        )
    } else {
        None
    };
    let download_dir = args
        .download_dir
        .as_deref()
        .unwrap_or_else(|| tmpdir.as_ref().expect("infallible").dir_path());

    if let Some(ref download_dir) = args.download_dir {
        if matches!(args.base, OtaPath::S3(_) | OtaPath::Version(_))
            || matches!(args.top, OtaPath::S3(_) | OtaPath::Version(_))
        {
            tokio::fs::create_dir_all(download_dir)
                .await
                .wrap_err("failed to create download_dir at ``")?;
        } else {
            warn!("--download-dir was specified but neither the base nor top OTA path requires downloading");
        }
    }

    let client = if args.base.is_local() && args.top.is_local() {
        None
    } else {
        let client = orb_s3_helpers::client()
            .await
            .wrap_err("failed to create s3 client")?;
        Some(client)
    };
    let base_path = crate::fetch::fetch_path(client.as_ref(), &args.base, download_dir)
        .await
        .wrap_err("failed to get base ota")?;
    info!("downloaded base ota at {}", base_path.display());
    let top_path = crate::fetch::fetch_path(client.as_ref(), &args.top, download_dir)
        .await
        .wrap_err("failed to get base ota")?;
    info!("downloaded top ota at {}", top_path.display());

    diff_ota(
        &base_path,
        &top_path,
        &args.out,
        cancel,
        !args.skip_input_hashing,
    )
    .await
    .wrap_err("failed to diff the two OTAs")
}

#[cfg(test)]
mod test {
    use super::*;
    use async_tempfile::TempDir;

    #[tokio::test]
    async fn test_empty_dir() {
        let empty = TempDir::new().await.unwrap();
        assert!(is_empty_dir(empty.dir_path())
            .await
            .expect("dir exists so reading should work"))
    }

    #[tokio::test]
    async fn test_populated_dir() {
        let populated = TempDir::new().await.unwrap();
        tokio::fs::create_dir(populated.dir_path().join("foo"))
            .await
            .unwrap();
        assert!(!is_empty_dir(populated.dir_path())
            .await
            .expect("dir exists so reading should work"))
    }

    #[tokio::test]
    async fn test_missing_dir() {
        let tmp = TempDir::new().await.unwrap();
        let missing = tmp.dir_path().join("missing");
        assert!(
            is_empty_dir(&missing).await.is_err(),
            "expected an error because dir doesn't exist"
        );
    }
}
