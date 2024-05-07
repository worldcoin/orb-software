#![forbid(unsafe_code)]

mod download_s3;
mod flash;

use build_info::{make_build_info, BuildInfo};
use camino::Utf8PathBuf;
use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use flash::FlashVariant;
use tracing::info;
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser, Debug)]
#[command(about, author, version=BUILD_INFO.git.describe, styles=make_clap_v3_styles())]
struct Cli {
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

fn current_dir() -> Utf8PathBuf {
    std::env::current_dir().unwrap().try_into().unwrap()
}

// No need to waste RAM with a threadpool.
#[tokio::main(flavor = "current_thread")]
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
            bail!("both rts_path and s3_url were specified - only provide one or the other");
        }
        if args.download_dir.is_some() {
            bail!("both rts_path and download_dir were specified - only provide one or the other");
        }
        info!("using already downloaded rts tarball");
        rts_path
    } else {
        bail!("you must provide either rts_path or s3_url");
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

fn make_clap_v3_styles() -> clap::builder::Styles {
    use clap::builder::styling::AnsiColor;
    clap::builder::Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
