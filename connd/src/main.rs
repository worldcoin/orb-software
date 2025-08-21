use color_eyre::eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    connd::run().await?;

    Ok(())
}
