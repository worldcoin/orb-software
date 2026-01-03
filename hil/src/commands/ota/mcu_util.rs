//! Parsing utilities for `orb-mcu-util` command output.

use color_eyre::{eyre::eyre, Result};

/// Parses MCU info output and checks if Main board current and secondary versions match.
/// Returns Ok(true) if versions match, Ok(false) if they don't match.
pub fn check_main_board_versions_match(output: &str) -> Result<bool> {
    let (current, secondary) = parse_board_versions(output, "Main board")?;

    Ok(current == secondary)
}

/// Parses MCU info output and checks if Security board current and secondary versions match.
/// Returns Ok(true) if versions match, Ok(false) if they don't match.
pub fn check_security_board_versions_match(output: &str) -> Result<bool> {
    let (current, secondary) = parse_board_versions(output, "Security board")?;

    Ok(current == secondary)
}

pub fn check_jetson_post_ota(output: &str) -> Result<bool> {
    // ensure that the `jetson` state is `STATUS_SUCCESS` & booted after ota update
    // (`ota 1`). Here is the output example:
    // ```
    // jetson       STATUS_SUCCESS                      booted (autoboot: ota 1, ram 0)
    // ```
    let jetson_state = output.lines().find(|line| line.starts_with("jetson"));
    if jetson_state.unwrap_or("").contains("ota 1") {
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn parse_board_versions(
    output: &str,
    pattern_section: &str,
) -> Result<(String, String)> {
    // Find the `pattern_section` section
    let mut in_board_section = false;
    let mut current_version: Option<String> = None;
    let mut secondary_version: Option<String> = None;

    for line in output.lines() {
        // Check if we're entering the board section
        if line.contains(pattern_section) {
            in_board_section = true;
            continue;
        }

        // Check if we're entering a different board section (exit board section)
        if in_board_section
            && !line.starts_with('\t')
            && !line.starts_with(' ')
            && !line.is_empty()
        {
            // We've left the board section
            break;
        }

        if in_board_section {
            if line.contains("current image:") {
                current_version = Some(extract_version_from_line(line)?);
            } else if line.contains("secondary slot:") {
                secondary_version = Some(extract_version_from_line(line)?);
            }
        }
    }

    let current = current_version
        .ok_or_else(|| eyre!("Could not find 'current image' for {pattern_section}"))?;
    let secondary = secondary_version.ok_or_else(|| {
        eyre!("Could not find 'secondary slot' for {pattern_section}")
    })?;

    Ok((current, secondary))
}

/// Extracts the version (e.g., "v3.2.15-0x5133a47a (prod)") from a line like "current image: v3.2.15-0x5133a47a (prod)"
fn extract_version_from_line(line: &str) -> Result<String> {
    // Split by colon and get the value part
    let version_full = line
        .split(':')
        .nth(1)
        .ok_or_else(|| eyre!("Invalid line format: {}", line))?
        .trim();

    Ok(version_full.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_main_board_versions() {
        let output = r#"
🔮 Orb info:
	revision:	Diamond_PVT
	power supply:	corded 🔌
	voltage:	14830mV
🚜 Main board:
	current image:	v3.2.15-0x5133a47a (prod)
	secondary slot:	v3.2.15-0x5133a47a (prod)
🔐 Security board:
	current image:	v3.2.15-0x0 (dev)
	secondary slot:	v3.2.15-0x0 (dev)
	battery charge:	100%
	voltage:	4130mV
	charging:	no
"#;

        let (current, secondary) = parse_board_versions(output, "Main board").unwrap();
        assert_eq!(current, "v3.2.15-0x5133a47a (prod)");
        assert_eq!(secondary, "v3.2.15-0x5133a47a (prod)");
        assert!(check_main_board_versions_match(output).unwrap());

        let (current, secondary) =
            parse_board_versions(output, "Security board").unwrap();
        assert_eq!(current, "v3.2.15-0x0 (dev)");
        assert_eq!(secondary, "v3.2.15-0x0 (dev)");
        assert!(check_security_board_versions_match(output).unwrap());
    }

    #[test]
    fn test_parse_main_board_versions_mismatch() {
        let output = r#"
🔮 Orb info:
	revision:	Diamond_PVT
	power supply:	corded 🔌
	voltage:	14830mV
🚜 Main board:
	current image:	v3.2.14-0x5133a47a (prod)
	secondary slot:	v3.2.15-0x2cc8ddfb (prod)
🔐 Security board:
	current image:	v3.2.15-0x0 (dev)
	secondary slot:	v3.2.15-0x2cc8ddfb (prod)
	battery charge:	100%
	voltage:	4130mV
	charging:	no
"#;

        let (current, secondary) = parse_board_versions(output, "Main board").unwrap();
        assert_eq!(current, "v3.2.14-0x5133a47a (prod)");
        assert_eq!(secondary, "v3.2.15-0x2cc8ddfb (prod)");
        assert!(!check_main_board_versions_match(output).unwrap());

        let (current, secondary) =
            parse_board_versions(output, "Security board").unwrap();
        assert_eq!(current, "v3.2.15-0x0 (dev)");
        assert_eq!(secondary, "v3.2.15-0x2cc8ddfb (prod)");
        assert!(!check_security_board_versions_match(output).unwrap());
    }
}
