#[cfg(feature = "orb-id")]
mod orb_id;
#[cfg(feature = "orb-jabil-id")]
mod orb_jabil_id;
#[cfg(feature = "orb-name")]
mod orb_name;
#[cfg(feature = "orb-token")]
mod orb_token;

#[cfg(feature = "orb-id")]
pub use orb_id::OrbId;
#[cfg(feature = "orb-jabil-id")]
pub use orb_jabil_id::OrbJabilId;
#[cfg(feature = "orb-name")]
pub use orb_name::OrbName;
#[cfg(feature = "orb-token")]
pub use orb_token::OrbToken;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrbInfoError {
    #[error("field is not yet available")]
    Unavailable,
    #[error(transparent)]
    IoErr(#[from] std::io::Error),
    #[error(transparent)]
    NotifyErr(#[from] notify::Error),
    #[error(transparent)]
    Utf8Err(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    OrbIdErr(#[from] hex::FromHexError),
    #[error(transparent)]
    ZbusErr(#[from] zbus::Error),
}
