use figment::providers::{Format, Toml};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::{borrow::Cow, path::Path};
use tracing::info;

use crate::{
    args::Args,
    orb_info::{get_orb_id, get_orb_token},
};

const CFG_DEFAULT_PATH: &str = "/etc/orb_fleet_cmdr.conf";
const ENV_VAR_PREFIX: &str = "ORB_FLEET_CMDR_";
const CFG_ENV_VAR: &str = const_format::concatcp!(ENV_VAR_PREFIX, "CONFIG");

#[serde_as]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Settings {
    pub orb_id: Option<String>,
    pub orb_token: Option<String>,
    pub relay_namespace: Option<String>,
}

impl Settings {
    /// Constructs `Settings` from a config file, environment variables, and command line
    /// arguments. Command line arguments always take precedence over environment variables, which
    /// in turn take precedence over the config file.
    pub async fn get(args: &Args) -> figment::error::Result<Settings> {
        let config_path = Self::get_config_source(args);

        let mut figment = figment::Figment::new();
        if config_path.exists() {
            figment = figment.merge(figment::providers::Toml::file(config_path));
        }
        if let Ok(orb_id) = get_orb_id().await {
            let orb_id_str = format!("orb_id={}", orb_id);
            figment =
                figment.merge(figment::providers::Data::<Toml>::string(&orb_id_str));
        }
        if let Ok(orb_token) = get_orb_token().await {
            let orb_token_str = format!("orb_token={}", orb_token);
            figment =
                figment.merge(figment::providers::Data::<Toml>::string(&orb_token_str));
        }

        figment
            .merge(figment::providers::Env::prefixed(ENV_VAR_PREFIX))
            .merge(figment::providers::Serialized::defaults(args))
            .extract()
    }

    fn get_config_source(args: &Args) -> Cow<'_, Path> {
        if let Some(config) = &args.config {
            info!("using config provided by command line argument: `{config}`");
            Cow::Borrowed(config.as_ref())
        } else if let Some(config) = figment::providers::Env::var(CFG_ENV_VAR) {
            info!("using config set in environment variable `{CFG_ENV_VAR}={config}`");
            Cow::Owned(std::path::PathBuf::from(config))
        } else {
            info!("using default config at `{CFG_DEFAULT_PATH}`");
            Cow::Borrowed(CFG_DEFAULT_PATH.as_ref())
        }
    }
}
