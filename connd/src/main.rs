use color_eyre::eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    orb_connd::run().await?;

    Ok(())
}
