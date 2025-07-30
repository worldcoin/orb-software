use std::{path::Path, sync::Arc};

use color_eyre::Result;
use iroh::Watcher;
use iroh::{protocol::Router, Endpoint};
use iroh_blobs::{store::fs::FsStore, ticket::BlobTicket, BlobsProtocol};

pub struct BlobNode {
    endpoint: Endpoint,
    store: Arc<FsStore>,
    _router: Router,
}

impl BlobNode {
    pub async fn start(store_location: impl AsRef<Path>) -> Result<Self> {
        let endpoint = Endpoint::builder().discovery_n0().bind().await?;

        let store = Arc::new(
            FsStore::load(store_location)
                .await
                .map_err(|e| color_eyre::eyre::eyre!(e))?,
        );

        let blobs = BlobsProtocol::new(&store, endpoint.clone(), None);

        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();

        Ok(Self {
            endpoint,
            store,
            _router: router,
        })
    }

    pub async fn import(&self, path: &Path) -> Result<BlobTicket> {
        let abs_path = std::fs::canonicalize(path)?;
        let tag = self.store.blobs().add_path(abs_path).await?;

        let addr = self.endpoint.node_addr().initialized().await?;

        Ok(BlobTicket::new(addr.clone(), tag.hash, tag.format))
    }

    pub async fn fetch(&self, ticket: BlobTicket, output_path: &Path) -> Result<()> {
        let downloader = self.store.downloader(&self.endpoint);

        self.endpoint.add_node_addr(ticket.node_addr().clone())?;

        downloader
            .download(ticket.hash(), Some(ticket.node_addr().node_id))
            .await
            .map_err(|e| color_eyre::eyre::eyre!(e))?;

        self.store
            .blobs()
            .export(ticket.hash(), output_path)
            .await?;
        Ok(())
    }
}
