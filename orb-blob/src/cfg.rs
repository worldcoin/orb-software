use color_eyre::{eyre::Context, Result};
use std::{env, path::PathBuf};

pub struct Cfg {
    pub port: u16,
    pub store_path: PathBuf,
    pub sqlite_path: PathBuf,
    /// The maxmimum amount of time we'll wait to reach the minimum amount of peers
    /// required before giving up.
    pub peer_listen_timeout_secs: u64,
    /// The minimum amount of peers required to start a download
    pub min_peer_req: usize,
}

impl Cfg {
    pub fn from_env() -> Result<Self> {
        let num_or = |env_var, default| {
            env::var(env_var)
                .ok()
                .map(|p| p.parse::<usize>())
                .transpose()
                .wrap_err_with(|| format!("could not parse {env_var}"))
                .map(|x| x.unwrap_or(default))
        };

        let port = num_or("ORB_BLOB_PORT", 8080)? as u16;
        let peer_listen_timeout_secs = num_or("ORB_BLOB_PEER_LISTEN_TIMEOUT", 60)? as u64;
        let min_peer_req = num_or("ORB_BLOB_MIN_PEER_REQ", 1)?;

        let store_path = env::var("ORB_BLOB_STORE_PATH")
            .wrap_err("ORB_BLOB_STORE_PATH must be provided")?;

        let sqlite_path = env::var("ORB_BLOB_SQLITE_PATH")
            .wrap_err("ORB_BLOB_SQLITE_PATH must be provided")?;

        Ok(Self {
            port,
            store_path: PathBuf::from(store_path),
            sqlite_path: PathBuf::from(sqlite_path),
            peer_listen_timeout_secs,
            min_peer_req,
        })
    }
}
