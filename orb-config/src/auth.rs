use std::path::Path;

use eyre::{Result, WrapErr};
use secrecy::SecretString;

// Replace the static token file when Android device attestation is available.
pub async fn read_token(path: &Path) -> Result<SecretString> {
    let token = SecretString::new(
        tokio::fs::read_to_string(path)
            .await
            .wrap_err("failed to read token file")?,
    );

    Ok(token)
}
