use color_eyre::{eyre::Context, Result};
use std::env;

#[derive(Debug)]
pub struct Cfg {
    pub client_id: String,
    pub auth_token: String,
    pub domain: String,
}

impl Cfg {
    pub fn from_env() -> Result<Cfg> {
        Ok(Cfg {
            client_id: env::var("ORB_RELAY_CLIENT_ID")
                .wrap_err("ORB_RELAY_AUTH_TOKEN is not set")?,

            auth_token: env::var("ORB_RELAY_AUTH_TOKEN")
                .wrap_err("ORB_RELAY_AUTH_TOKEN is not set")?,

            domain: env::var("ORB_RELAY_DOMAIN")
                .unwrap_or_else(|_| "relay.stage.orb.worldcoin.org".to_string()),
        })
    }
}
