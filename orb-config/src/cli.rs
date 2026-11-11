use std::path::PathBuf;

use clap::Parser;
use orb_info::OrbId;
use reqwest::Url;

use super::BUILD_INFO;

#[derive(Parser)]
#[command(version = BUILD_INFO.version, about)]
pub(crate) struct Args {
    /// Full HTTPS URL of the backend.
    #[arg(long, value_parser = parse_endpoint)]
    pub endpoint: Url,
    /// Orb ID used as the HTTP Basic authentication username.
    #[arg(long)]
    pub orb_id: OrbId,
    /// File containing the static backend authentication token.
    #[arg(long)]
    pub token_file: PathBuf,
    /// Socket exposed by the local Zenoh router.
    #[arg(long, default_value = "/dev/socket/zenohd.sock")]
    pub zenoh_socket: PathBuf,
}

fn parse_endpoint(value: &str) -> Result<Url, String> {
    let endpoint = Url::parse(value).map_err(|error| error.to_string())?;
    if endpoint.scheme() != "https" {
        return Err("configuration endpoint must use HTTPS".into());
    }
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err(
            "use --token-file instead of credentials in the endpoint URL".into(),
        );
    }
    if endpoint.fragment().is_some() {
        return Err("configuration endpoint must not contain a URL fragment".into());
    }
    Ok(endpoint)
}
