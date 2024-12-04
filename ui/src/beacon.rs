use crate::engine::Engine;
use eyre::Result;
use std::time::Duration;
use tokio::time;
use tracing::info;

pub async fn beacon(ui: &dyn Engine, duration: Duration) -> Result<()> {
    info!("🔹 Starting beacon");

    let end_time = time::Instant::now() + duration;
    while time::Instant::now() < end_time {
        ui.beacon();
        time::sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}
