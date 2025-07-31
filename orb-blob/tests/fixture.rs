#![allow(dead_code)]
use async_tempfile::{TempDir, TempFile};
use color_eyre::Result;
use orb_blob::{cfg::Cfg, program};
use std::{net::SocketAddr, path::PathBuf};
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
}

impl Fixture {
    pub async fn new() -> Self {
        let sqlite = TempFile::new().await.unwrap();
        let blob_store = TempDir::new().await.unwrap();
        let listener = TcpListener::bind("0.0.0.0:0000").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cfg = Cfg {
            port: addr.port(),
            sqlite_path: PathBuf::from(sqlite.file_path()),
            store_path: PathBuf::from(blob_store.dir_path()),
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
        }
    }

    pub async fn stop_server(&mut self) {
        self.cancel_token.cancel();
        self.server_handle.take().unwrap().await.unwrap().unwrap();
    }
}
