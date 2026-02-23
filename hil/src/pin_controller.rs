use color_eyre::Result;

/// Trait for controlling power and recovery pins on hardware devices.
pub trait PinController {
    /// Press the power button for the specified duration.
    ///
    /// If duration is None, the button remains pressed (caller must ensure it's released).
    /// If duration is Some, the button is pressed for that duration then released.
    fn press_power_button(
        &mut self,
        duration: Option<std::time::Duration>,
    ) -> Result<()>;

    /// Control the recovery mode pin.
    ///
    /// - `true`: Recovery mode enabled (device boots into recovery)
    /// - `false`: Normal boot mode
    fn set_recovery(&mut self, enabled: bool) -> Result<()>;

    /// Reset the controller hardware state.
    fn reset(&mut self) -> Result<()>;

    /// Turn off the device by pressing the power button.
    fn turn_off(&mut self) -> Result<()>;

    /// Turn on the device by pressing the power button.
    fn turn_on(&mut self) -> Result<()>;

    /// Destroy the controller, resetting hardware state.
    fn destroy(&mut self) -> Result<()>;
}
