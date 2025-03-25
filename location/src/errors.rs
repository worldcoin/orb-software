use thiserror::Error;

#[derive(Error, Debug)]
pub enum LocationError {
    #[error("WiFi scanning error: {0}")]
    WiFiScanError(String),

    #[error("Cellular scanning error: {0}")]
    CellScanError(String),

    #[error("Network operation timed out after {0} seconds")]
    OperationTimeout(u64),

    #[error("Backend communication error: {0}")]
    BackendError(String),

    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Token unavailable: {0}")]
    TokenError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

// Result type alias for functions in this crate
pub type Result<T> = std::result::Result<T, LocationError>;
