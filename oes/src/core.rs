use serde::{Deserialize, Serialize};

/// Service started event published to `oes/service_started`.
#[derive(Serialize, Deserialize)]
pub struct ServiceStartedEvent {}

/// Bootstrap outcome published to `oes/bootstrap`, before the signup flow starts.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BootstrapEvent {
    Failed { stage: BootstrapStage },
    Succeeded,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapStage {
    Network,
    Token,
    Configuration,
    Warmup,
}

/// A validated persisted operator credential was adopted, not merely read.
#[derive(Serialize, Deserialize)]
pub struct OperatorQrRestoredEvent {}

/// A QR scan event, recording the current phase and outcome.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::must_use_candidate)]
pub struct QrScanEvt {
    /// Which scanning phase produced this event.
    phase: QrScanPhase,
    /// The outcome of the scan attempt.
    state: QrScanState,
    /// How an accepted operator QR was supplied. Legacy events omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<QrScanOrigin>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrScanOrigin {
    Camera,
    Cli,
    #[serde(other)]
    Unknown,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn operator_qr_origin_is_optional_and_forward_compatible() {
        let legacy = json!({
            "phase": "operator",
            "state": {"success": {"kind": "operator"}}
        });
        let event: QrScanEvt = from_value(legacy.clone()).unwrap();
        assert_eq!(event.origin, None);
        assert_eq!(to_value(event).unwrap(), legacy);

        for (origin, expected) in [
            ("camera", QrScanOrigin::Camera),
            ("cli", QrScanOrigin::Cli),
            ("future_input", QrScanOrigin::Unknown),
        ] {
            let mut payload = legacy.clone();
            payload["origin"] = json!(origin);
            let event: QrScanEvt = from_value(payload).unwrap();
            assert_eq!(event.origin, Some(expected));
        }
    }

    #[test]
    fn bootstrap_failure_requires_a_bounded_stage() {
        for stage in ["network", "token", "configuration", "warmup"] {
            let payload = json!({"state": "failed", "stage": stage});
            let event: BootstrapEvent = from_value(payload.clone()).unwrap();
            assert_eq!(to_value(event).unwrap(), payload);
        }
        assert!(from_value::<BootstrapEvent>(json!({"state": "failed"})).is_err());
        assert!(from_value::<BootstrapEvent>(
            json!({"state": "failed", "stage": "unexpected"})
        )
        .is_err());
        let recovered = json!({"state": "succeeded"});
        assert_eq!(
            from_value::<BootstrapEvent>(recovered.clone()).unwrap(),
            BootstrapEvent::Succeeded
        );
        assert_eq!(to_value(BootstrapEvent::Succeeded).unwrap(), recovered);
    }

    #[test]
    fn restoration_has_no_credential_payload() {
        assert_eq!(to_value(OperatorQrRestoredEvent {}).unwrap(), json!({}));
        assert!(from_value::<OperatorQrRestoredEvent>(json!({})).is_ok());
        assert!(from_value::<OperatorQrRestoredEvent>(json!("credential")).is_err());
    }
}
