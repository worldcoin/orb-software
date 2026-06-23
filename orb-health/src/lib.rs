use tracing::warn;

pub mod file_sizes;

pub async fn run() {
    if let Err(e) = file_sizes::run().await {
        warn!("orb-health::file_sizes failed: {e}");
    }
}
