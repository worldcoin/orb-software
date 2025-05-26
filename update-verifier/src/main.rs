use orb_update_verifier::run;
use tracing::error;

const SYSLOG_IDENTIFIER: &str = "worldcoin-update-verifier";

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let result =
        run().inspect_err(|error| error!(?error, "failed to run update-verifier"));

    telemetry.flush_blocking();

    result
}
