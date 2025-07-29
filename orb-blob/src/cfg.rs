use std::env;

use color_eyre::eyre::Context;

pub struct Cfg {
    port: usize,
}

impl Cfg {
    pub fn from_env() {
        let port = env::var("ORB_BLOB_PORT")
            .ok()
            .and_then(|p| p.parse::<usize>().ok())
            .unwrap_or(8080);

        Self { port }
    }
}
