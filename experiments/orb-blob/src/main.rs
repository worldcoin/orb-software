use color_eyre::Result;
use orb_blob::{cfg::Cfg, program};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald("worldcoin-blob")
        .init();

    let cfg = Cfg::from_env()?;
    let listener = TcpListener::bind(format!("127.0.0.1:{}", cfg.port)).await?;
    let cancel_token = CancellationToken::new();
    let result = program::run(cfg, listener, cancel_token).await;

    tel_flusher.flush().await;
    result
}
