use color_eyre::eyre::{Result, WrapErr as _};
use orb_const_concat::const_concat;

use crate::BUILD_INFO;

const USER_AGENT: &str = const_concat!(
    "orb-se050-reprovision/",
    BUILD_INFO.cargo.pkg_version,
    "-",
    BUILD_INFO.git.describe,
);

#[derive(Debug, Clone)]
pub struct Client(pub reqwest::Client);

impl Client {
    pub fn new() -> Result<Self> {
        Ok(Self(
            orb_security_utils::reqwest::http_client_builder()
                .user_agent(USER_AGENT)
                .build()
                .wrap_err("failed to create http client")?,
        ))
    }
}
