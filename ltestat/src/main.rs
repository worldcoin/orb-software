use color_eyre::eyre::{ContextCompat, Result};
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<()> {
    ltestat::run().await?;

    Ok(())
}
