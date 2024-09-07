//! A common health check module.

pub mod mcu;
pub mod teleport;

use tracing::{info, instrument};

/// A common health check trait.
pub trait Check {
    type Error;

    /// Name of module.
    const NAME: &'static str;

    /// Perform the actual health check for a module.
    fn check(&self) -> Result<(), Self::Error> {
        Ok(())
    }

    #[instrument(fields(module=Self::NAME), skip_all)]
    fn run_check(&self) -> Result<(), Self::Error> {
        info!("performing health check for {}", Self::NAME);
        self.check()?;
        info!("health check succeeded");
        Ok(())
    }
}
