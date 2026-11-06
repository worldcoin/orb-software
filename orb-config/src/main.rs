#![forbid(unsafe_code)]

mod auth;
mod backend;
mod cli;
mod publish;
mod service;

use clap::Parser;
use eyre::{eyre, Result, WrapErr};
use orb_build_info::{make_build_info, BuildInfo};
use orb_telemetry::TelemetryConfig;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use zenorb::Zenorb;

const BUILD_INFO: BuildInfo = make_build_info!();

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::Args::parse();
    let telemetry = TelemetryConfig::new();
    #[cfg(target_os = "android")]
    let telemetry = telemetry.with_logcat(c"orb-config");

    let telemetry = match telemetry.try_init() {
        Ok(telemetry) => Some(telemetry),
        Err(error) => {
            eprintln!("failed to initialize orb-config telemetry: {error}");
            None
        }
    };

    tracing::info!(version = BUILD_INFO.version, "starting orb-config");
    let result = program(args).await;
    if let Err(error) = &result {
        tracing::error!(%error, "orb-config failed to start");
    }

    if let Some(telemetry) = telemetry {
        telemetry.flush().await;
    }

    result
}

async fn program(args: cli::Args) -> Result<()> {
    let token = auth::read_token(&args.token_file).await?;
    let client = orb_security_utils::reqwest::client_builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .wrap_err("failed to create configuration HTTP client")?;

    let socket = args
        .zenoh_socket
        .to_str()
        .ok_or_else(|| eyre!("Zenoh socket path must be UTF-8"))?;
    let mut zenoh = zenorb::default_cfg();
    for (key, value) in [
        (
            "connect/endpoints",
            serde_json::json!([format!("unixsock-stream/{socket}")]),
        ),
        ("connect/timeout_ms", serde_json::json!(5000)),
    ] {
        zenoh
            .insert_json5(key, &value.to_string())
            .map_err(|error| eyre!("invalid Zenoh configuration: {error}"))?;
    }

    let session = Zenorb::from_cfg(zenoh)
        .orb_id(args.orb_id.clone())
        .with_name("orb-config")
        .await
        .wrap_err("failed to open Zenoh session")?;
    let publisher = publish::new(session)?;

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    tokio::select! {
        _ = service::run(client, args.endpoint, args.orb_id, token, publisher) => {},
        _ = terminate.recv() => tracing::info!("received SIGTERM"),
        _ = interrupt.recv() => tracing::info!("received SIGINT"),
    }
    Ok(())
}
