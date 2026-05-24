use dogstatsd::{Client, DogstatsdResult, Options};
use once_cell::sync::Lazy;

/// Orb identification code.
pub static DATADOG: Lazy<Client> = Lazy::new(init_datadog_client);

/// Removes the need to put` &[] as &[&str]` everywhere.
pub const NO_TAGS: &[&str] = &[];

fn init_datadog_client() -> Client {
    let datadog_options = Options::default();

    match Client::new(datadog_options) {
        Ok(client) => client,
        Err(err) => {
            tracing::error!(
                "failed to initialize datadog telemetry client: {err:?}"
            );
            Client::disabled()
        }
    }
}

/// A trait for logging errors instead of propagating the error with `?`.
pub trait LogOnError {
    /// Logs an error message to the default logger at the `Error` level.
    fn or_log(&self);
}

impl LogOnError for DogstatsdResult {
    fn or_log(&self) {
        if let Err(e) = self {
            tracing::error!("Datadog reporting failed with error: {e:#?}");
        }
    }
}
