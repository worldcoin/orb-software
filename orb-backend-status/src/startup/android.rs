use super::Config;
use clap::Parser;
use color_eyre::{eyre::Context, Result};
use orb_backend_status::{collectors, BUILD_INFO};
use orb_info::{OrbJabilId, OrbName};
use reqwest::Url;
use std::path::PathBuf;

// TODO: Use OrbId::read() once the Android orb-info implementation is available.
const TEMP_ORB_ID: &str = "00000000";

#[derive(Parser)]
#[command(version = BUILD_INFO.version, about = "Forward Android OES events to the backend")]
pub struct Args {
    /// Full backend status URL.
    #[arg(long)]
    endpoint: Url,
    /// File containing the temporary backend authentication token.
    #[arg(long)]
    token_file: PathBuf,
    /// Socket exposed by the separate Zenoh router.
    #[arg(long, default_value = "/data/local/tmp/zenohd.sock")]
    zenoh_socket: String,
    /// Socket exposed by the local DogStatsD agent.
    #[arg(long, default_value = "/data/local/tmp/dsd.socket")]
    metrics_socket: String,
    /// Platform version to report to the backend.
    #[arg(long, default_value = "unknown")]
    orb_os_version: String,
}

fn zenoh_config(socket: &str) -> Result<zenorb::zenoh::Config> {
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

pub async fn configure(args: Args) -> Result<Config> {
    let token = tokio::fs::read_to_string(&args.token_file)
        .await
        .wrap_err_with(|| {
            format!("failed to read token file {}", args.token_file.display())
        })?;

    Ok(Config {
        orb_id: TEMP_ORB_ID.parse().expect("temporary orb ID must be valid"),
        orb_name: OrbName("unknown".to_owned()),
        orb_jabil_id: OrbJabilId("unknown".to_owned()),
        orb_os_version: args.orb_os_version,
        endpoint: args.endpoint,
        zenoh: zenoh_config(&args.zenoh_socket)?,
        collectors: collectors::Config {
            token: token.trim().to_owned(),
        },
        metrics_socket: Some(args.metrics_socket),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_identity_is_supported_by_current_orb_info() {
        let orb_id: orb_info::OrbId = TEMP_ORB_ID.parse().unwrap();
        assert_eq!(orb_id.as_str(), "00000000");
    }

    #[test]
    fn requires_backend_url_and_token_file() {
        assert!(Args::try_parse_from(["orb-backend-status"]).is_err());
        assert!(Args::try_parse_from([
            "orb-backend-status",
            "--endpoint",
            "https://example.com/status",
        ])
        .is_err());
    }

    #[test]
    fn accepts_explicit_startup_configuration() {
        let args = Args::try_parse_from([
            "orb-backend-status",
            "--endpoint",
            "https://example.com/status",
            "--token-file",
            "/data/local/tmp/token",
            "--zenoh-socket",
            "/data/local/tmp/custom.sock",
            "--orb-os-version",
            "android-test",
        ])
        .unwrap();
        assert_eq!(args.endpoint.as_str(), "https://example.com/status");
        assert_eq!(args.token_file, PathBuf::from("/data/local/tmp/token"));
        assert_eq!(args.orb_os_version, "android-test");
        let config = zenoh_config(&args.zenoh_socket).unwrap();
        assert_eq!(
            config.get_json("connect/endpoints").unwrap(),
            r#"["unixsock-stream//data/local/tmp/custom.sock"]"#
        );
    }

    #[tokio::test]
    async fn loads_static_token_and_temporary_identity() {
        let dir = async_tempfile::TempDir::new().await.unwrap();
        let token_file = dir.to_path_buf().join("token");
        tokio::fs::write(&token_file, "test-token\n").await.unwrap();
        let args = Args {
            endpoint: "https://example.com/status".parse().unwrap(),
            token_file,
            zenoh_socket: "/data/local/tmp/zenohd.sock".to_owned(),
            metrics_socket: "/data/local/tmp/dsd.socket".to_owned(),
            orb_os_version: "android-test".to_owned(),
        };
        let config = configure(args).await.unwrap();
        assert_eq!(config.orb_id.as_str(), "00000000");
        assert_eq!(config.collectors.token, "test-token");
        assert_eq!(config.orb_os_version, "android-test");
    }

    #[tokio::test]
    async fn unreadable_token_file_has_actionable_error() {
        let dir = async_tempfile::TempDir::new().await.unwrap();
        let args = Args {
            endpoint: "https://example.com/status".parse().unwrap(),
            token_file: dir.to_path_buf().join("missing-token"),
            zenoh_socket: "/data/local/tmp/zenohd.sock".to_owned(),
            metrics_socket: "/data/local/tmp/dsd.socket".to_owned(),
            orb_os_version: "unknown".to_owned(),
        };
        let error = configure(args)
            .await
            .err()
            .expect("missing token must fail");
        assert!(error.to_string().contains("failed to read token file"));
        assert!(error.to_string().contains("missing-token"));
    }
}
