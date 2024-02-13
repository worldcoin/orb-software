#[tokio::main]
async fn main() -> eyre::Result<()> {
    orb_short_lived_token_daemon::main().await
}
