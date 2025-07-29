use color_eyre::{eyre::Context, Result};
use std::{env, path::PathBuf};

pub struct Cfg {
    pub port: u16,
    pub store_path: PathBuf,
    pub sqlite_path: PathBuf,
}

impl Cfg {
    pub fn from_env() -> Result<Self> {
        let port = env::var("ORB_BLOB_PORT")
            .ok()
            .map(|p| p.parse::<u16>())
            .transpose()
            .wrap_err("could not parse ORB_BLOB_PORT")?
            .unwrap_or(8080);

        let store_path = env::var("ORB_BLOB_STORE_PATH")
            .wrap_err("ORB_BLOB_STORE_PATH must be provided")?;

        let sqlite_path = env::var("ORB_BLOB_SQLITE_PATH")
            .wrap_err("ORB_BLOB_SQLITE_PATH must be provided")?;

        Ok(Self {
            port,
            store_path: PathBuf::from(store_path),
            sqlite_path: PathBuf::from(sqlite_path),
        })
    }
}
