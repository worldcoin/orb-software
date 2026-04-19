use serde::{Deserialize, Serialize};

/// Service started event published to `oes/service_started`.
#[derive(Serialize, Deserialize)]
pub struct ServiceStartedEvent {}

/// A QR scan event, recording the current phase and outcome.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::must_use_candidate)]
pub struct QrScanEvt {
    /// Which scanning phase produced this event.
    phase: QrScanPhase,
    /// The outcome of the scan attempt.
    state: QrScanState,
}

/// Outcome of a QR scan attempt.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrScanState {
    /// Still scanning for a QR code.
    Scanning,
    /// A valid QR code was scanned successfully.
    Success {
        /// The type of QR code that was scanned.
        kind: QrScanType,
    },
    /// Generic error.
    Err(String),
    /// Network connectivity issues prevented the scan from completing.
    NetworkIssues,
    /// The scanned QR code was not recognized.
    Invalid,
    /// The scan timed out without a valid QR code.
    Timeout,
    /// The scan was cancelled because it was no longer needed.
    Cancelled {
        /// Optional reason describing why the scan was cancelled.
        reason: Option<String>,
    },
    /// The scan or subsequent validation failed.
    FailedValidation {
        /// The type of QR code that failed validation.
        kind: QrScanType,
        /// Details about the validation failure.
        failure: QrScanValidationFailure,
    },
    /// Issues talking with orb relay.
    UserDataNotReceived,
}

/// Which QR scanning mode the orb is in.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrScanPhase {
    /// Scanning for an operator QR code.
    Operator,
    /// Scanning for a user QR code.
    User,
    /// Scanning for a WiFi or Netconfig QR code.
    Conn,
}

/// What type of QR code was actually scanned.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrScanType {
    /// A normal user QR code.
    User,
    /// An operator QR code.
    Operator,
    /// A magic QR code for special actions.
    Magic,
    /// A safety QR code.
    Safety,
    /// A connection QR code (WiFi or network config).
    Conn,
    /// A signup extension / data acquisition QR code.
    SignupExtension,
    /// The user paired via the orb-app relay instead of scanning a QR code.
    RelayPaired,
}

/// Category of a QR scan or validation failure.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrScanValidationFailure {
    /// No location data was available for validation.
    NoLocationData,
    /// The QR code content was invalid.
    Invalid(String),
    /// An unknown validation failure occurred.
    Unknown,
    /// A network error prevented validation.
    Network,
    /// The backend returned an internal server error.
    InternalServerError,
    /// Appointment verification timed out.
    VerifyAppointmentTimeout,
    /// Could not connect to the appointment verification service.
    VerifyAppointmentConnectionFailed,
    /// Appointment verification failed.
    VerifyAppointmentFailed,
    /// The appointment verification response was invalid.
    VerifyAppointmentInvalidResponse,
    /// The bypass-age token was invalid.
    VerifyBypassAgeTokenInvalid,
    /// The hash of the QR code did not match.
    HashMismatch,
}

/// Subset of orb-core config published via zenoh.
///
/// Add new fields here to expose them to backend-status and other services.
/// All fields are optional so only explicitly set values are published.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PublishableConfig {
    /// Whether the thermal camera is required for signup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thermal_camera_required: Option<bool>,
}
