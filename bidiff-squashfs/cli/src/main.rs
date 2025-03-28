use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

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
use orb_bidiff_squashfs_cli::{
    diff_ota::diff_ota, file_or_stdout::stdout_if_none, is_empty_dir, ota_path::OtaPath,
};
use orb_build_info::{make_build_info, BuildInfo};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Debug, Parser)]
#[clap(
    author,
    about,
    version = BUILD_INFO.version,
    styles = clap_v3_styles(),
)]
enum Args {
    Diff(DiffCommand),
    Patch(PatchCommand),
    Ota(OtaCommand),
}

#[derive(Debug, Parser)]
struct DiffCommand {
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

#[derive(Debug, Parser)]
struct PatchCommand {
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

#[derive(Debug, Parser)]
struct OtaCommand {
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
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let telemetry_flusher = orb_telemetry::TelemetryConfig::new().init();

    let result = match args {
        Args::Diff(c) => tokio::task::spawn_blocking(|| run_diff(c))
            .await
            .wrap_err("task panicked")
            .and_then(|r| r),
        Args::Patch(c) => tokio::task::spawn_blocking(|| run_patch(c))
            .await
            .wrap_err("task panicked")
            .and_then(|r| r),
        Args::Ota(c) => {
            let cancel = CancellationToken::new();
            // we only attempt to handle ctrl-c for async tasks, blocking tasks can't
            // actually be cancelled like that.
            tokio::task::spawn(handle_ctrlc(cancel.clone()));

            cancel
                .run_until_cancelled(run_mk_ota(c, cancel.clone()))
                .await
                .unwrap_or(Ok(())) // unwrap if cancelled
        }
    };
    telemetry_flusher.flush().await;

    result
}

fn run_diff(args: DiffCommand) -> Result<()> {
    // TODO: instead of reading the entire file, it may make sense to memmap large files
    let base_contents = fs::read(&args.base).wrap_err("failed to read base file")?;
    let top_contents = fs::read(&args.top).wrap_err("failed to read top file")?;
    let mut out_writer =
        io::BufWriter::new(stdout_if_none(args.out.as_deref(), false)?);
    orb_bidiff_squashfs::diff_squashfs()
        .old_path(&args.base)
        .old(&base_contents)
        .new_path(&args.top)
        .new(&top_contents)
        .out(&mut out_writer)
        .diff_params(&DiffParams::default())
        .call()
        .wrap_err("failed to perform diff")?;
    out_writer
        .into_inner()
        .wrap_err("failed to flush buffered writer")?
        .flush()
        .wrap_err("failed to flush file")
}

fn run_patch(args: PatchCommand) -> Result<()> {
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
    let mut out_writer = io::BufWriter::new(
        stdout_if_none(args.out.as_deref(), args.force_overwrite_file)
            .wrap_err("failed to open out file")?,
    );

    let mut patch_processor = bipatch::Reader::new(patch_reader, base_reader)
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

async fn run_mk_ota(args: OtaCommand, cancel: CancellationToken) -> Result<()> {
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
    let base_path = orb_bidiff_squashfs_cli::fetch::fetch_path(
        client.as_ref(),
        &args.base,
        download_dir,
    )
    .await
    .wrap_err("failed to get base ota")?;
    info!("downloaded base ota at {}", base_path.display());
    let top_path = orb_bidiff_squashfs_cli::fetch::fetch_path(
        client.as_ref(),
        &args.top,
        download_dir,
    )
    .await
    .wrap_err("failed to get base ota")?;
    info!("downloaded top ota at {}", top_path.display());

    diff_ota(&base_path, &top_path, &args.out, cancel)
        .await
        .wrap_err("failed to diff the two OTAs")
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

async fn handle_ctrlc(cancel: CancellationToken) {
    let _guard = cancel.drop_guard();
    let _ = tokio::signal::ctrl_c().await;
}
