use color_eyre::eyre::{Result, WrapErr};
use secrecy::SecretString;

use crate::orb_info::get_orb_token;

#[derive(Debug)]
pub struct Settings {
    pub orb_id: Option<String>,
    pub orb_token: Option<SecretString>,
    pub relay_namespace: Option<String>,
}

impl Settings {
    /// Constructs `Settings` from a config file, environment variables, and command line
    /// arguments. Command line arguments always take precedence over environment variables, which
    /// in turn take precedence over the config file.
    pub async fn get() -> Result<Settings> {
        let orb_id =
            std::env::var("ORB_ID").wrap_err("env variable `ORB_ID` should be set")?;
        let relay_namespace = std::env::var("RELAY_NAMESPACE")
            .wrap_err("env variable `RELAY_NAMESPACE` should be set")?;

        let orb_token = get_orb_token().await?;

        Ok(Settings {
            orb_id: Some(orb_id),
            orb_token: Some(SecretString::new(orb_token)),
            relay_namespace: Some(relay_namespace),
        })
    }
}
