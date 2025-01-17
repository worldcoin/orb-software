use tracing::{debug, error, info, trace, warn};

fn main() {
    // if you don't care about flushing all logs, you can ignore the return value.
    let _ = orb_telemetry::TelemetryConfig::new().init();

    trace!("TRACE");
    debug!("DEBUG");
    info!("INFO");
    warn!("WARN");
    error!("ERROR");
}
