use clap::Parser;
use color_eyre::eyre::Result;
use orb_endpoints::{backend::Backend, endpoints::Endpoints, OrbId};
use orb_fleet_cmdr::{
    args::Args, handlers::OrbCommandHandlers, relay_connect, relay_disconnect,
    settings::Settings,
};
use orb_relay_messages::orb_commands::v1::OrbCommandIssue;
use std::{str::FromStr, time::Duration};
use tokio::signal::unix::SignalKind;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

const SYSLOG_IDENTIFIER: &str = "worldcoin-fleet-cmdr";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();
    let settings = Settings::get(&args).await?;

    debug!(?settings, "starting fleet commander with settings");
    run(&settings).await
}

async fn run(settings: &Settings) -> Result<()> {
    info!("running fleet commander: {:?}", settings);

    let orb_id = OrbId::from_str(settings.orb_id.as_ref().unwrap())?;
    let endpoints = Endpoints::new(Backend::from_env()?, &orb_id);
    let shutdown_token = CancellationToken::new();

    let mut relay =
        relay_connect(settings, &endpoints, 3, Duration::from_secs(10)).await?;

    let handlers = OrbCommandHandlers::new();

    let mut sighup = tokio::signal::unix::signal(SignalKind::hangup())?;
    let mut sigint = tokio::signal::unix::signal(SignalKind::interrupt())?;
    let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())?;
    let mut sigquit = tokio::signal::unix::signal(SignalKind::quit())?;

    loop {
        tokio::select! {
            _ = sighup.recv() => {
                info!("hangup detected");
                shutdown_token.cancel();
            }
            _ = sigint.recv() => {
                info!("sigint detected");
                shutdown_token.cancel();
            }
            _ = sigterm.recv() => {
                info!("sigterm detected");
                shutdown_token.cancel();
            }
            _ = sigquit.recv() => {
                info!("sigquit detected");
                shutdown_token.cancel();
            }
            _ = shutdown_token.cancelled() => {
                info!("shutting down fleet commander");
                break;
            }
            msg = relay.wait_for_msg::<OrbCommandIssue>(Duration::MAX) => {
                match msg {
                    Ok(command) => {
                        info!("received command: {:?}", command);
                        match handlers.handle_orb_command(command).await {
                            Ok(res) => {
                                match relay.send(res).await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        error!("error sending command result: {:?}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("error handling command: {:?}", e);
                                match relay.send(e).await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        error!("error sending command error: {:?}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("error receiving command: {:?}", e);
                        break;
                    }
                }
            }
        }
    }

    relay_disconnect(&mut relay, Duration::from_secs(10), Duration::from_secs(10))
        .await?;

    Ok(())
}
