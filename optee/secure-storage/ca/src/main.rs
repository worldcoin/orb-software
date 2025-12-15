#![forbid(unsafe_code)]

use std::io::{Read, Write};

use clap::Parser;
use color_eyre::Result;
use eyre::Context;
use orb_secure_storage_ca::Client;

const SYSLOG_IDENTIFIER: &str = "ORB_SECURE_STORAGE_CA";

fn main() -> Result<()> {
    color_eyre::install()?;
    let flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();
    let args = Args::parse();
    args.run()?;
    flusher.flush_blocking();

    Ok(())
}

#[derive(Debug, Parser)]
enum Args {
    Get(GetArgs),
    Put(PutArgs),
}

impl Args {
    fn run(self) -> Result<()> {
        match self {
            Self::Get(args) => args.run(),
            Self::Put(args) => args.run(),
        }
    }
}

#[derive(Debug, Parser)]
struct GetArgs {
    key: String,
}

impl GetArgs {
    fn run(self) -> Result<()> {
        let mut client =
            Client::new().wrap_err("failed to create secure storage client")?;
        let val = client.get(&self.key).wrap_err("failed CA get")?;

        let mut stdout = std::io::stdout();
        stdout
            .write_all(&val)
            .and_then(|()| stdout.flush())
            .wrap_err("failed to write to stdout")?;

        Ok(())
    }
}

#[derive(Debug, Parser)]
struct PutArgs {
    key: String,
}

impl PutArgs {
    fn run(self) -> Result<()> {
        let mut client =
            Client::new().wrap_err("failed to create secure storage client")?;
        let mut stdin = std::io::stdin();
        let mut value = Vec::new();
        stdin
            .read_to_end(&mut value)
            .wrap_err("failed to read from stdin")?;
        let oldval = client.put(&self.key, &value).wrap_err("failed CA put")?;
        let mut stdout = std::io::stdout();
        stdout
            .write_all(&oldval)
            .wrap_err("failed to write to stdout")?;

        Ok(())
    }
}
