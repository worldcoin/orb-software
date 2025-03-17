mod file_or_stdout;

use std::{
    fs,
    io::{self, Write as _},
    path::PathBuf,
};

use bidiff::DiffParams;
use clap::Parser;
use clap_stdin::FileOrStdin;
use color_eyre::{eyre::WrapErr as _, Result};
use tracing::info;

use crate::file_or_stdout::stdout_if_none;

#[derive(Debug, Parser)]
#[clap(about, author)]
enum Args {
    Diff(DiffCommand),
    Patch(PatchCommand),
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

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let telemetry_flusher = orb_telemetry::TelemetryConfig::new().init();

    let result = match args {
        Args::Diff(c) => run_diff(c),
        Args::Patch(c) => run_patch(c),
    };
    telemetry_flusher.flush_blocking();

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
