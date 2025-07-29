use color_eyre::{eyre::Context, Result};
use std::env;

pub struct Cfg {
    pub port: u16,
}

impl Cfg {
    pub fn from_env() -> Result<Self> {
        let port = env::var("ORB_BLOB_PORT")
            .ok()
            .map(|p| p.parse::<u16>())
            .transpose()
            .wrap_err("could not parse ORB_BLOB_PORT")?
            .unwrap_or(8080);

        Ok(Self { port })
    }
}
