use std::sync::Arc;

use color_eyre::Result;
use iroh::{protocol::Router, Endpoint};
use iroh_blobs::{store::mem::MemStore, BlobsProtocol};

pub struct BlobNode {
    _endpoint: Endpoint,
    _store: Arc<MemStore>,
    _router: Router,
}

impl BlobNode {
    pub async fn start() -> Result<Self> {
        let endpoint = Endpoint::builder().discovery_n0().bind().await?;

        let store = Arc::new(MemStore::new());

        let blobs = BlobsProtocol::new(&store, endpoint.clone(), None);

        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();

        Ok(Self {
            _endpoint: endpoint,
            _store: store,
            _router: router,
        })
    }
}
