use std::time::Duration;

use tracing::{debug, error, info, instrument, trace, warn};

const SERVICE_NAME: &str = "my-service";
const SERVICE_VERSION: &str = "v1.2.3"; // get this from orb-build-info instead

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_opentelemetry(orb_telemetry::OpentelemetryConfig::new(
            orb_telemetry::OpentelemetryAttributes {
                service_name: SERVICE_NAME.to_string(),
                service_version: SERVICE_VERSION.to_string(),
                additional_otel_attributes: Default::default(),
            },
        )?)
        .with_journald(SERVICE_NAME)
        .init();

    trace!("TRACE");
    debug!("DEBUG");
    info!("INFO");
    warn!("WARN");
    error!("ERROR");

    some_longer_task(69);

    telemetry.flush_blocking();

    Ok(())
}

#[instrument]
fn some_longer_task(arg: u8) {
    std::thread::sleep(Duration::from_millis(1000));
    info!("got result: {arg}");
}
