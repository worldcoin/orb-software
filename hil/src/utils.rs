use color_eyre::Result;
use std::time::Duration;

/// Custom duration parser to allow flexible time formats
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if s.ends_with("ms") {
        let value: u64 = s
            .trim_end_matches("ms")
            .parse()
            .map_err(|_| color_eyre::eyre::eyre!("Invalid duration format: {}", s))?;
        Ok(Duration::from_millis(value))
    } else if s.ends_with('s') {
        let value: u64 = s
            .trim_end_matches('s')
            .parse()
            .map_err(|_| color_eyre::eyre::eyre!("Invalid duration format: {}", s))?;
        Ok(Duration::from_secs(value))
    } else if s.ends_with('m') {
        let value: u64 = s
            .trim_end_matches('m')
            .parse()
            .map_err(|_| color_eyre::eyre::eyre!("Invalid duration format: {}", s))?;
        Ok(Duration::from_secs(value * 60))
    } else {
        // Default to seconds if no unit is provided
        let value: u64 = s
            .parse()
            .map_err(|_| color_eyre::eyre::eyre!("Invalid duration format: {}", s))?;
        Ok(Duration::from_secs(value))
    }
}
