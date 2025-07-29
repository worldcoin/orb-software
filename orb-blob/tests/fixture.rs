use color_eyre::Result;
use orb_blob::{cfg::Cfg, program};
use std::net::SocketAddr;
use tokio::{
    net::TcpListener,
    task::{self, JoinHandle},
};

pub struct Fixture {
    pub _server_handle:JoinHandle<Result<()>>,
    pub addr: SocketAddr,
}

impl Fixture {
    pub async fn new() -> Self {
        let listener = TcpListener::bind("0.0.0.0:0000").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cfg = Cfg { port: addr.port() };
        let _server_handle = task::spawn(program::run(cfg, listener));

        Self {
            _server_handle,
            addr,
        }
    }
}
