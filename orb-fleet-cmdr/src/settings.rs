use std::path::{Path, PathBuf};

use figment::providers::Format;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::args::Args;

#[serde_as]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Settings {
    pub orb_relay_url: String,
    pub orb_name_path: PathBuf,
}

impl Settings {
    /// Constructs `Settings` from a config file, environment variables, and command line
    /// arguments. Command line arguments always take precedence over environment variables, which
    /// in turn take precedence over the config file.
    pub fn get<P: AsRef<Path>>(
        args: &Args,
        config: P,
        env_prefix: &str,
    ) -> figment::error::Result<Settings> {
        figment::Figment::new()
            .merge(figment::providers::Toml::file(config))
            .merge(figment::providers::Env::prefixed(env_prefix))
            .merge(figment::providers::Serialized::defaults(args))
            .extract()
    }
}
