use color_eyre::Result;
use tokio::task::{self, JoinHandle};

pub async fn ensure_connectivity() -> JoinHandle<Result<()>> {
    task::spawn(async move { Ok::<_, color_eyre::Report>(()) })
}
