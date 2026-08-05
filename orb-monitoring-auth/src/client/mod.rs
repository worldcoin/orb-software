//! Implements the Datadog external-auth client for monitoring credentials.
//!
//! The applet adapts process startup to `main`; `main` validates the caller,
//! reads the token from the Unix socket, and writes the external-auth response
//! to the caller-supplied destination.

use crate::{server, Token};
use clap::ArgMatches;
use color_eyre::{
    eyre::{bail, ContextCompat},
    Result,
};
use nix::libc::getuid;
use prelude::applet::Applet;
use serde_json::json;
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    process::ExitCode,
};
use tracing::{error, warn};

#[derive(Debug, Default)]
pub struct OrbMonitoringAuthClient;

impl Applet for OrbMonitoringAuthClient {
    fn name(&self) -> &'static str {
        "orb-monitoring-auth-client"
    }

    fn about(&self) -> &'static str {
        "client used by DataDog to fetch the monitoring token"
    }

    fn main(&self, _args: &ArgMatches) -> ExitCode {
        match main(
            crate::DD_AGENT_UID,
            server::DEFAULT_SOCKET,
            std::io::stdout(),
        ) {
            Err(e) => {
                error!("failed with {e}");
                ExitCode::FAILURE
            }

            Ok(_) => ExitCode::SUCCESS,
        }
    }
}

/// Fetches the monitoring token and writes the Datadog external-auth response.
///
/// `dd_agent_uid` is the only UID allowed to invoke the client.
/// `token_server_socket` identifies the server's Unix socket.
/// `output` receives one JSON response followed by a newline.
///
/// The function returns an error when client setup, UID validation, socket I/O,
/// response serialization, or output writing fails. A missing token is encoded
/// in the response's `error` field.
pub fn main(
    dd_agent_uid: u32,
    token_server_socket: impl AsRef<Path>,
    mut output: impl Write,
) -> Result<()> {
    if let Err(error) = color_eyre::install() {
        warn!("failed to install color-eyre error hook: {error}");
    }

    unsafe {
        let uid = getuid();
        if uid != dd_agent_uid {
            bail!(
                "user with id {uid} is not allowed to call orb-monitoring-auth-client"
            )
        }
    }

    let (value, error) = match fetch_monitoring_token(token_server_socket.as_ref()) {
        Err(e) => (
            serde_json::Value::Null,
            serde_json::Value::String(e.to_string()),
        ),

        Ok(t) => (serde_json::Value::String(t.token), serde_json::Value::Null),
    };

    let response = json!({
        "monitoring_token": {
            "value": value,
            "error": error
        }
    });

    writeln!(output, "{response}")?;

    Ok(())
}

fn fetch_monitoring_token(socket: &Path) -> Result<Token> {
    let mut stream = UnixStream::connect(socket)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;

    let token: Option<Token> = serde_json::from_slice(&response)?;
    token.wrap_err("there is no token available")
}
