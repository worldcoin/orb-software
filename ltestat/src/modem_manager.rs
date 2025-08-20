use crate::utils::{retrieve_value, run_cmd};
use color_eyre::{eyre::ContextCompat, Result};

pub struct ModemInfo {
    pub modem_id: String,
    pub imei: String,
    pub iccid: String,
}

pub async fn get_modem_info() -> Result<ModemInfo> {
    let output = run_cmd("mmcli", &["-L"]).await?;
    let modem_id = output
        .split_whitespace()
        .next()
        .and_then(|path| path.rsplit('/').next())
        .map(|s| s.to_owned())
        .wrap_err("Failed to get modem id")?;
    // If we manage to get modem, grab the iccid and imei next
    let output = run_cmd("mmcli", &["-m", &modem_id, "--output-keyvalue"]).await?;

    let imei = retrieve_value(&output, "modem.generic.equipment-identifier")?;

    let sim_output = run_cmd("mmcli", &["-i", "0", "--output-keyvalue"]).await?;

    let iccid = retrieve_value(&sim_output, "sim.properties.iccid")?;

    Ok(ModemInfo {
        modem_id,
        imei,
        iccid,
    })
}
