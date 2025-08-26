use eyre::Result;

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
    let versions_json = read_versions_json();
    match read_current_slot().as_str() {
        "a" => Ok(versions_json.releases.slot_a),
        "b" => Ok(versions_json.releases.slot_b),
        slot => bail!("Unexpected slot: {slot}"),
    }
}

#[cfg(not(test))]
fn read_versions_json() -> VersionsJson {
    let versions_str = std::fs::read_to_string(VERSIONS_PATH)
        .expect("couldn't read versions.json file");
    serde_json::from_str(&versions_str)
        .expect("couldn't deserialize versions.json file")
}

#[cfg(not(test))]
fn read_current_slot() -> String {
    match std::env::var(CURRENT_BOOT_SLOT)
        .expect("Could not read the current boot slot environment variable")
    {
        s if s.is_empty() => {
            panic!("CURRENT_BOOT_SLOT environmental variable is empty")
        }
        other => other,
    }
}

#[cfg(test)]
pub fn orb_os_version() -> Result<String> {
    Ok("test".into())
}
