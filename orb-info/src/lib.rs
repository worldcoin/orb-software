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
#[non_exhaustive]
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
    #[cfg(feature = "orb-token")]
    #[error(transparent)]
    ZbusErr(#[from] zbus::Error),
}

pub async fn from_file(path: &str) -> Result<String, OrbInfoError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s.trim().to_string()),
        Err(e) => Err(OrbInfoError::IoErr(e)),
    }
}

pub async fn from_env(env_var: &str) -> Result<String, OrbInfoError> {
    match std::env::var(env_var) {
        Ok(s) => Ok(s.trim().to_string()),
        Err(_) => Err(OrbInfoError::Unavailable),
    }
}

pub async fn from_binary(path: &str) -> Result<String, OrbInfoError> {
    let output = tokio::process::Command::new(path)
        .output()
        .await
        .map_err(OrbInfoError::IoErr)?;
    match output.status.success() {
        true => match String::from_utf8(output.stdout) {
            Ok(s) => Ok(s.trim().to_string()),
            Err(e) => Err(OrbInfoError::Utf8Err(e)),
        },
        false => Err(OrbInfoError::IoErr(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("{} binary failed", path),
        ))),
    }
}
