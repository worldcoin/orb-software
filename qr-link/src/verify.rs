use orb_relay_messages::common::v1::AppAuthenticatedData;

use crate::{QR_VERSION_4, QR_VERSION_5};

/// Error returned by [`verify_qr`] for unrecognized QR versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedVersion(pub u8);

impl std::fmt::Display for UnsupportedVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported QR version: {}", self.0)
    }
}

impl std::error::Error for UnsupportedVersion {}

/// Verifies an `AppAuthenticatedData` hash using the verification method
/// corresponding to the given QR version.
pub fn verify_qr(
    app_data: &AppAuthenticatedData,
    hash: &[u8],
    version: u8,
) -> Result<bool, UnsupportedVersion> {
    match version {
        QR_VERSION_4 => Ok(app_data.verify(hash)),
        QR_VERSION_5 => Ok(app_data.verify_with_length_prefix(hash)),
        _ => Err(UnsupportedVersion(version)),
    }
}
