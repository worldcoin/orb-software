//! A common health check module.

pub mod mcu;

use tracing::{info, instrument};

/// A common health check trait.
pub trait Check {
    type Error;

    /// Name of module.
    const NAME: &'static str;

    #[allow(dead_code)]
    /// Perform the actual health check for a module.
    fn check(&self) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Perfom current/expected MCU version check on current slot
    fn check_current(&self) -> Result<(), Self::Error> {
        Ok(())
    }

    #[instrument(fields(module=Self::NAME), skip_all)]
    fn run_check(&self) -> Result<(), Self::Error> {
        info!("performing health check for {}", Self::NAME);
        self.check_current()?;
        info!("health check succeeded");
        Ok(())
    }
}
