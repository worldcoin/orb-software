#[cfg(feature = "orb-id")]
pub mod orb_id;
#[cfg(feature = "orb-jabil-id")]
pub mod orb_jabil_id;
#[cfg(feature = "orb-name")]
pub mod orb_name;
#[cfg(feature = "orb-os-release")]
pub mod orb_os_release;
#[cfg(feature = "orb-token")]
pub mod orb_token;

use std::io;
use std::path::Path;
use std::process::Output;

#[cfg(feature = "orb-id")]
pub use orb_id::OrbId;
#[cfg(feature = "orb-jabil-id")]
pub use orb_jabil_id::OrbJabilId;
#[cfg(feature = "orb-name")]
pub use orb_name::OrbName;
#[cfg(feature = "orb-token")]
pub use orb_token::TokenTaskHandle;

#[cfg(feature = "async")]
async fn from_file(path: impl AsRef<Path>) -> io::Result<String> {
    tokio::fs::read_to_string(path)
        .await
        .map(|s| s.trim().to_owned())
}

#[cfg_attr(any(feature = "orb-name", feature = "orb-jabil-id"), expect(dead_code))]
fn from_file_blocking(path: impl AsRef<Path>) -> io::Result<String> {
    std::fs::read_to_string(path).map(|s| s.trim().to_owned())
}

#[cfg(feature = "async")]
#[allow(dead_code)]
async fn from_binary(path: impl AsRef<Path>) -> io::Result<String> {
    let output = tokio::process::Command::new(path.as_ref()).output().await?;
    from_binary_output(output, path)
}

#[allow(dead_code)]
fn from_binary_blocking(path: impl AsRef<Path>) -> io::Result<String> {
    let output = std::process::Command::new(path.as_ref()).output()?;
    from_binary_output(output, path)
}

#[allow(dead_code)]
fn from_binary_output(output: Output, path: impl AsRef<Path>) -> io::Result<String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(std::io::Error::other(format!(
            "{} binary failed",
            path.as_ref().display()
        )))
    }
}
