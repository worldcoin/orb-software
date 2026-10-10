use super::Config;
use clap::Parser;
use color_eyre::{
    eyre::{ensure, eyre, Context},
    Result,
};
use orb_backend_status::{collectors, BUILD_INFO};
use orb_endpoints::{v2::Endpoints, Backend};
use orb_info::{OrbId, OrbJabilId, OrbName};
use secrecy::{ExposeSecret, SecretString};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(version = BUILD_INFO.version, about = "Forward Android OES events to the backend")]
pub struct Args {
    /// Backend environment: prod, staging, or analysis.
    #[arg(long)]
    backend: Backend,
    /// File containing the temporary backend authentication token.
    #[arg(long)]
    token_file: PathBuf,
    /// Socket exposed by the separate Zenoh router.
    #[arg(long, default_value = "/dev/socket/zenohd.sock")]
    zenoh_socket: PathBuf,
    /// Socket exposed by the local DogStatsD agent.
    #[arg(long, default_value = "/dev/socket/datadog.socket")]
    metrics_socket: PathBuf,
    /// Platform version to report to the backend.
    #[arg(long, default_value = "unknown")]
    orb_os_version: String,
}

fn zenoh_config(socket: &Path) -> Result<zenorb::zenoh::Config> {
    let socket = socket
        .to_str()
        .ok_or_else(|| eyre!("Zenoh socket path must be valid UTF-8"))?;
    let mut config = zenorb::default_cfg();
    config
        .insert_json5(
            "connect/endpoints",
            &serde_json::to_string(&[format!("unixsock-stream/{socket}")])?,
        )
        .map_err(|e| {
            color_eyre::eyre::eyre!("invalid Zenoh socket configuration: {e}")
        })?;
    Ok(config)
}

pub async fn configure(args: Args, orb_id: OrbId) -> Result<Config> {
    ensure!(
        args.backend != Backend::Local,
        "local backend is not supported; use prod, staging, or analysis"
    );
    let endpoint = Endpoints::new(args.backend, &orb_id).status;
    let zenoh = zenoh_config(&args.zenoh_socket)?;
    let metrics_socket = args
        .metrics_socket
        .into_os_string()
        .into_string()
        .map_err(|_| eyre!("metrics socket path must be valid UTF-8"))?;
    let token = {
        let contents = SecretString::new(
            tokio::fs::read_to_string(&args.token_file)
                .await
                .wrap_err_with(|| {
                    format!("failed to read token file {}", args.token_file.display())
                })?,
        );
        SecretString::new(contents.expose_secret().trim().to_owned())
    };
    ensure!(
        !token.expose_secret().is_empty(),
        "token file {} is empty or whitespace-only; provide a non-empty backend authentication token before starting",
        args.token_file.display()
    );

    Ok(Config {
        orb_id,
        orb_name: OrbName::read_unfallable().await,
        orb_jabil_id: OrbJabilId("unknown".to_owned()),
        orb_os_version: args.orb_os_version,
        endpoint,
        zenoh,
        collectors: collectors::Config { token },
        metrics_socket: Some(metrics_socket),
    })
}
