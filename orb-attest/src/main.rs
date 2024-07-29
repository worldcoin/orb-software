#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    orb_attest::main().await
}
