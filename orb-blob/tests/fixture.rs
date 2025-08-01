#![allow(dead_code)]
use async_tempfile::{TempDir, TempFile};
use bon::bon;
use color_eyre::Result;
use iroh::{PublicKey, SecretKey};
use orb_blob::{cfg::Cfg, program};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use tokio::{
    net::TcpListener,
    task::{self, JoinHandle},
};
use tokio_util::sync::CancellationToken;

pub struct Fixture {
    server_handle: Option<JoinHandle<Result<()>>>,
    pub addr: SocketAddr,
    _sqlite_store: TempFile,
    pub blob_store: TempDir,
    cancel_token: CancellationToken,
    pub public_key: PublicKey,
}

#[bon]
impl Fixture {
    #[builder]
    #[builder(on(String, into))]
    pub async fn new(
        #[builder(default = 1)] min_peer_req: usize,
        #[builder(default=Duration::from_secs(30))] peer_listen_timeout: Duration,
        well_known_nodes: Vec<PublicKey>,
        secret_key: Option<SecretKey>,
        #[builder(default = true)] local: bool,
    ) -> Self {
        let sqlite = TempFile::new().await.unwrap();
        let blob_store = TempDir::new().await.unwrap();
        let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let secret_key = secret_key.unwrap_or_else(|| {
            let mut rng = rand::rngs::OsRng;
            SecretKey::generate(&mut rng)
        });

        let public_key = secret_key.public();

        let cfg = Cfg {
            port: addr.port(),
            sqlite_path: PathBuf::from(sqlite.file_path()),
            store_path: PathBuf::from(blob_store.dir_path()),
            peer_listen_timeout,
            min_peer_req,
            secret_key,
            well_known_nodes,
            iroh_local: local,
        };

        let cancel_token = CancellationToken::new();
        let server_handle =
            task::spawn(program::run(cfg, listener, cancel_token.clone()));

        Self {
            server_handle: Some(server_handle),
            addr,
            _sqlite_store: sqlite,
            blob_store,
            cancel_token,
            public_key,
        }
    }

    pub async fn stop_server(&mut self) {
        self.cancel_token.cancel();
        self.server_handle.take().unwrap().await.unwrap().unwrap();
    }
}
