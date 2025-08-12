pub mod lte_data;
pub mod utils;

use color_eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    Ok(())
}
