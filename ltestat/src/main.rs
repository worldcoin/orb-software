pub mod lte_data;

use color_eyre::Result;
use tokio::process::Command;

fn main() -> Result<()> {
    color_eyre::install()?;
    Ok(())
}
