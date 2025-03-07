#![forbid(unsafe_code)]

mod boot;
mod commands;
mod download_s3;
mod flash;
mod ftdi;
mod models;
mod serial;

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use color_eyre::{eyre::WrapErr, Result};
use orb_build_info::{make_build_info, BuildInfo};
use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser, Debug)]
#[command(about, author, version=BUILD_INFO.version, styles=make_clap_v3_styles())]
struct Cli {
    #[command(subcommand)]
    commands: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    ButtonCtrl(crate::commands::ButtonCtrl),
    Cmd(crate::commands::Cmd),
    Flash(crate::commands::Flash),
    Login(crate::commands::Login),
    Reboot(crate::commands::Reboot),
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
            Commands::ButtonCtrl(c) => c.run().await,
            Commands::Cmd(c) => c.run().await,
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
