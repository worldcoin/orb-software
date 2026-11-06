use super::Config;
use android_system_properties::AndroidSystemProperties;
use clap::Parser;
use color_eyre::{
    eyre::{ensure, eyre, Context},
    Result,
};
use orb_backend_status::{collectors, BUILD_INFO};
use orb_endpoints::{v2::Endpoints, Backend};
use orb_info::{OrbId, OrbJabilId, OrbName};
use secrecy::{ExposeSecret, SecretString};
use std::path::PathBuf;
use tracing::warn;

#[derive(Parser)]
#[command(version = BUILD_INFO.version, about = "Forward Android OES events to the backend")]
pub struct Args {
    /// File containing the temporary backend authentication token.
    #[arg(long)]
    token_file: PathBuf,
}

fn zenoh_config() -> Result<zenorb::zenoh::Config> {
    zenorb::zenoh::Config::from_json5(
        r#"{
            mode: "client",
            connect: { endpoints: ["unixsock-stream//dev/socket/zenohd/zenohd.sock"] },
            listen: { endpoints: [] },
            scouting: {
                multicast: { enabled: false },
                gossip: { enabled: false },
            },
        }"#,
    )
    .map_err(|e| eyre!("invalid Android Zenoh configuration: {e}"))
}

pub async fn configure(args: Args, orb_id: OrbId) -> Result<Config> {
    let orb_os_version = tokio::task::spawn_blocking(|| {
        AndroidSystemProperties::new().get("ro.build.fingerprint")
    })
    .await
    .wrap_err("Android property reader task failed")?
    .filter(|version| !version.trim().is_empty())
    .unwrap_or_else(|| {
        warn!("Android build fingerprint unavailable or empty. Using [unknown]");
        String::from("unknown")
    });

    let endpoint = Endpoints::new(Backend::Staging, &orb_id).status;
    let zenoh = zenoh_config()?;
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
        orb_os_version,
        endpoint,
        zenoh,
        collectors: collectors::Config { token },
        metrics_socket: Some(
            "/data/vendor/datadog-agent-tfh/run/dsd.socket".to_owned(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_token_file_is_required() {
        let args = Args::try_parse_from([
            "orb-backend-status",
            "--token-file",
            "/data/vendor/orb-backend-status/backend-token",
        ])
        .unwrap();
        assert_eq!(
            args.token_file,
            PathBuf::from("/data/vendor/orb-backend-status/backend-token")
        );
        assert!(Args::try_parse_from(["orb-backend-status"]).is_err());
    }

    #[test]
    fn zenoh_connects_only_to_local_router() {
        let config = serde_json::to_value(zenoh_config().unwrap()).unwrap();
        assert_eq!(config["mode"], "client");
        assert_eq!(
            config["connect"]["endpoints"],
            serde_json::json!(["unixsock-stream//dev/socket/zenohd/zenohd.sock"])
        );
        assert_eq!(config["listen"]["endpoints"], serde_json::json!([]));
        assert_eq!(config["scouting"]["multicast"]["enabled"], false);
        assert_eq!(config["scouting"]["gossip"]["enabled"], false);
    }
}
