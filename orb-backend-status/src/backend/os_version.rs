use eyre::{Result, WrapErr};

#[cfg(not(test))]
use eyre::bail;
#[cfg(not(test))]
use serde::Deserialize;

#[cfg(not(test))]
#[derive(Deserialize)]
pub struct SlotReleases {
    pub slot_a: String,
    pub slot_b: String,
}

#[cfg(not(test))]
const CURRENT_BOOT_SLOT: &str = "CURRENT_BOOT_SLOT";
#[cfg(not(test))]
const VERSIONS_PATH: &str = "/usr/persistent/versions.json";

#[cfg(not(test))]
#[derive(Deserialize)]
pub struct VersionsJson {
    pub releases: SlotReleases,
}

#[cfg(not(test))]
pub fn orb_os_version() -> Result<String> {
    let versions_json = read_versions_json()?;
    match read_current_slot()?.as_str() {
        "a" => Ok(versions_json.releases.slot_a),
        "b" => Ok(versions_json.releases.slot_b),
        slot => bail!("Unexpected slot: {slot}"),
    }
}

#[cfg(not(test))]
fn read_versions_json() -> Result<VersionsJson> {
    let versions_str = std::fs::read_to_string(VERSIONS_PATH)
        .wrap_err("couldn't read versions.json file")?;
    let versions_json = serde_json::from_str(&versions_str)
        .wrap_err("couldn't deserialize versions.json file")?;

    Ok(versions_json)
}

#[cfg(not(test))]
fn read_current_slot() -> Result<String> {
    let slot = std::env::var(CURRENT_BOOT_SLOT)
        .wrap_err("Could not read the current boot slot environment variable")?;
    if slot.is_empty() {
        bail!("CURRENT_BOOT_SLOT environmental variable is empty");
    }

    Ok(slot)
}

#[cfg(test)]
pub fn orb_os_version() -> Result<String> {
    Ok("test".into())
}
