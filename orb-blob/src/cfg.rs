use color_eyre::{eyre::Context, Result};
use iroh::node_info::NodeIdExt;
use iroh::{PublicKey, SecretKey};
use std::{env, path::PathBuf, time::Duration};

pub struct Cfg {
    pub port: u16,
    pub store_path: PathBuf,
    pub sqlite_path: PathBuf,
    /// The maxmimum amount of time we'll wait to reach the minimum amount of peers
    /// required before giving up.
    pub peer_listen_timeout: Duration,
    /// The minimum amount of peers required to start a download
    pub min_peer_req: usize,
    pub secret_key: SecretKey,
    pub well_known_nodes: Vec<PublicKey>,
    /// If true, does not use relay, only uses mdns discovery, only binds on localhost
    pub iroh_local: bool,
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
        let flag = |env_var| env::var(env_var).is_ok();

        let port = num_or("ORB_BLOB_PORT", 8080)? as u16;
        let peer_listen_timeout = num_or("ORB_BLOB_PEER_LISTEN_TIMEOUT", 60)
            .map(|x| Duration::from_secs(x as u64))?;
        let min_peer_req = num_or("ORB_BLOB_MIN_PEER_REQ", 1)?;

        let store_path = env::var("ORB_BLOB_STORE_PATH")
            .wrap_err("ORB_BLOB_STORE_PATH must be provided")?;

        let sqlite_path = env::var("ORB_BLOB_SQLITE_PATH")
            .wrap_err("ORB_BLOB_SQLITE_PATH must be provided")?;

        let secret_key_raw = env::var("ORB_BLOB_SECRET_KEY");
        let secret_key = match secret_key_raw {
            Ok(s) => SecretKey::from_bytes(s.as_bytes().try_into()?),
            Err(_) => {
                let mut rng = rand::rngs::OsRng;
                SecretKey::generate(&mut rng)
            }
        };

        let well_known_nodes = env::var("ORB_BLOB_WELL_KNOWN_NODES")
            .unwrap_or_default()
            .split(",")
            .filter(|s| !s.is_empty())
            .map(PublicKey::from_z32)
            .collect::<Result<Vec<_>, _>>()
            .wrap_err("failed to decode well known nodes")?;

        let iroh_local = flag("ORB_BLOB_IROH_LOCAL");

        Ok(Self {
            port,
            store_path: PathBuf::from(store_path),
            sqlite_path: PathBuf::from(sqlite_path),
            peer_listen_timeout,
            min_peer_req,
            secret_key,
            well_known_nodes,
            iroh_local,
        })
    }
}
