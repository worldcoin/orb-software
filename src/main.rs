mod telemetry;

use crate::telemetry::ExecContext;
use tracing::{error, metadata::LevelFilter};

fn main() {
    if let Err(error) = telemetry::start::<ExecContext, _>(LevelFilter::INFO, std::io::stdout) {
        error!(
            ?error,
            "update verifier encountered error while starting telemetry"
        );
        std::process::exit(1);
    }

    if let Err(error) = update_verifier::run_health_check() {
        error!(
            ?error,
            "update verifier encountered error while checking system health"
        );
        std::process::exit(1);
    }
    std::process::exit(0);
}
