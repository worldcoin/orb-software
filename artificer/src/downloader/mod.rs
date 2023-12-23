//! Download functionality for various sources of artifacts.

use color_eyre::{eyre::WrapErr, Result};
use octocrab::Octocrab;

pub mod github;

#[derive(Debug, Clone)]
pub struct Client {
    pub octo: Octocrab,
    pub reqwest: reqwest::Client,
    pub gh_token: Option<String>,
}

impl Client {
    pub fn new(gh_token: Option<String>) -> Result<Self> {
        let b = Octocrab::builder();
        let b = if let Some(token) = gh_token.clone() {
            b.personal_token(token)
        } else {
            b
        };

        let octo = b.build().wrap_err("failed to initialize github api")?;

        let reqwest = ::reqwest::Client::builder()
            .user_agent("worldcoin/artificer")
            .build()
            .wrap_err("failed to build reqwest client")?;

        Ok(Self {
            octo,
            reqwest,
            gh_token,
        })
    }
}
