use std::{path::PathBuf, time::Duration};

use clap::Parser;
use color_eyre::{eyre::Context as _, Result};
use humantime::parse_duration;
use tracing::info;

use crate::{remote_cmd::CopyDirection, OrbConfig, RemoteArgs, RemoteTransport};

/// Copy a local file to the Orb.
#[derive(Debug, Parser)]
pub struct CopyTo {
    /// Path to the local file to copy.
    #[arg(long)]
    local: PathBuf,

    /// Destination path on the Orb.
    #[arg(long)]
    orb: PathBuf,

    /// Transport to use for the copy
    #[arg(long, value_enum, default_value_t = RemoteTransport::Ssh)]
    transport: RemoteTransport,

    /// Timeout duration (e.g., "60s", "2m")
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    timeout: Duration,

    #[command(flatten)]
    remote: RemoteArgs,
}

/// Copy a file from the Orb to the local machine.
#[derive(Debug, Parser)]
pub struct CopyFrom {
    /// Source path on the Orb.
    #[arg(long)]
    orb: PathBuf,

    /// Destination path on the local machine.
    #[arg(long)]
    local: PathBuf,

    /// Transport to use for the copy
    #[arg(long, value_enum, default_value_t = RemoteTransport::Ssh)]
    transport: RemoteTransport,

    /// Timeout duration (e.g., "60s", "2m")
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    timeout: Duration,

    #[command(flatten)]
    remote: RemoteArgs,
}

impl CopyTo {
    pub async fn run(self, orb_config: &OrbConfig) -> Result<()> {
        let session = self
            .remote
            .connect(self.transport, self.timeout, orb_config)
            .await?;
        info!(
            "Copying {} -> orb:{}",
            self.local.display(),
            self.orb.display()
        );
        tokio::time::timeout(
            self.timeout,
            session.copy_file(&self.local, &self.orb, CopyDirection::Upload),
        )
        .await
        .wrap_err("copy timed out")?
        .wrap_err("copy failed")
    }
}

impl CopyFrom {
    pub async fn run(self, orb_config: &OrbConfig) -> Result<()> {
        let session = self
            .remote
            .connect(self.transport, self.timeout, orb_config)
            .await?;
        info!(
            "Copying orb:{} -> {}",
            self.orb.display(),
            self.local.display()
        );
        tokio::time::timeout(
            self.timeout,
            session.copy_file(&self.local, &self.orb, CopyDirection::Download),
        )
        .await
        .wrap_err("copy timed out")?
        .wrap_err("copy failed")
    }
}
