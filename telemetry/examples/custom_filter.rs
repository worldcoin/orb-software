use std::time::Duration;

use tracing::{
    debug, error, info, instrument, level_filters::LevelFilter, trace, warn,
};
use tracing_subscriber::{filter::Targets, EnvFilter};

const SERVICE_NAME: &str = "my-service";
const SERVICE_VERSION: &str = "v1.2.3"; // You should use orb-build-info in your code

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(SERVICE_NAME)
        .with_opentelemetry(
            orb_telemetry::OpentelemetryConfig::new(
                orb_telemetry::OpentelemetryAttributes {
                    service_name: SERVICE_NAME.to_string(),
                    service_version: SERVICE_VERSION.to_string(),
                    additional_otel_attributes: Default::default(),
                },
            )?
            .with_filter(
                "reqwest=WARN,http=WARN" // filter out noisy events and spans
                    .parse::<Targets>()?
                    // still send everything else
                    .with_default(LevelFilter::TRACE),
            ),
        )
        // NOTE: this replaces the default global filter.
        .with_global_filter(
            // We still want users to be able to override the filter, so we allow
            // overriding the defaults with `RUST_LOG`
            std::env::var("RUST_LOG")
                // we specify INFO as the fallback/default verbosity. "custom_filter"
                // is the name of this example crate.
                .unwrap_or("info,custom_filter=debug".to_owned())
                .parse::<EnvFilter>()?,
        )
        .init();

    trace!("TRACE");
    debug!("DEBUG");
    info!("INFO");
    warn!("WARN");
    error!("ERROR");

    some_longer_task(69).await;

    telemetry.flush().await;
    Ok(())
}

#[instrument]
async fn some_longer_task(arg: u8) {
    tokio::time::sleep(Duration::from_millis(1000)).await;
    info!("got result: {arg}");
}
