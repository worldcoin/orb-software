use async_tempfile::TempFile;
use color_eyre::Result;
use orb_blob::{cfg::Cfg, program};
use std::{net::SocketAddr, path::PathBuf};
use tokio::{
    net::TcpListener,
    task::{self, JoinHandle},
};

pub struct Fixture {
    pub _server_handle: JoinHandle<Result<()>>,
    pub addr: SocketAddr,
    _sqlite_store: TempFile,
    _blob_store: TempFile,
}

impl Fixture {
    pub async fn new() -> Self {
        let sqlite = TempFile::new().await.unwrap();
        let blob_store = TempFile::new().await.unwrap();
        let listener = TcpListener::bind("0.0.0.0:0000").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cfg = Cfg {
            port: addr.port(),
            sqlite_path: PathBuf::from(sqlite.file_path()),
            store_path: PathBuf::from(blob_store.file_path()),
        };

        let _server_handle = task::spawn(program::run(cfg, listener));

        Self {
            _server_handle,
            addr,
            _sqlite_store: sqlite,
            _blob_store: blob_store
        }
    }
}
