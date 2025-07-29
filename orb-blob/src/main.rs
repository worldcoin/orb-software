use color_eyre::Result;
use orb_blob::{cfg::Cfg, program};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald("worldcoin-blob")
        .init();

    let cfg = Cfg::from_env()?;
    let listener = TcpListener::bind(format!("0.0.0.0:{}", cfg.port)).await?;
    let result = program::run(cfg, listener).await;

    tel_flusher.flush().await;
    result
}
