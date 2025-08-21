use color_eyre::eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    ltestat::run().await?;

    Ok(())
}
