use super::{
    connection_state::ConnectionState, location::MmcliLocationRoot,
    signal::MmcliSignalRoot,
};
use crate::utils::{retrieve_value, run_cmd};
use color_eyre::{
    eyre::{eyre, ContextCompat},
    Result,
};

pub async fn get_modem_id() -> Result<String> {
    let output = run_cmd("mmcli", &["-L"]).await?;
    let modem_id = output
        .split_whitespace()
        .next()
        .and_then(|path| path.rsplit('/').next())
        .map(|s| s.to_owned())
        .wrap_err("Failed to get modem id")?;

    Ok(modem_id)
}

pub async fn get_imei(modem_id: &str) -> Result<String> {
    let output = run_cmd("mmcli", &["-m", modem_id, "--output-keyvalue"]).await?;
    let imei = retrieve_value(&output, "modem.generic.equipment-identifier")?;

    Ok(imei)
}

pub async fn get_iccid() -> Result<String> {
    let sim_output = run_cmd("mmcli", &["-i", "0", "--output-keyvalue"]).await?;
    let iccid = retrieve_value(&sim_output, "sim.properties.iccid")?;

    Ok(iccid)
}

pub async fn get_connection_state(modem_id: &str) -> Result<ConnectionState> {
    let output = run_cmd("mmcli", &["-m", modem_id, "-K"]).await?;

    for line in output.lines() {
        if let Some(connection_line) = line.strip_prefix("modem.generic.state") {
            let data = connection_line
                .split(':')
                .nth(1)
                .ok_or_else(|| eyre!("Invalid modem.generic.state line format"))?
                .trim()
                .trim_matches('\'')
                .to_lowercase();

            return Ok(ConnectionState::from(data));
        }
    }

    Err(eyre!("modem.generic.state not found in mmcli output"))
}

pub async fn get_operator_and_rat(modem_id: &str) -> Result<(String, String)> {
    let output = run_cmd("mmcli", &["-m", modem_id, "--output-keyvalue"]).await?;

    let operator: String = retrieve_value(&output, "modem.3gpp.operator-name")?;

    let rat: String =
        retrieve_value(&output, "modem.generic.access-technologies.value[1] ")?;

    Ok((operator, rat))
}

pub async fn start_signal_refresh(modem_id: &str) -> Result<()> {
    run_cmd("mmcli", &["-m", modem_id, "--signal-setup", "10"]).await?;

    Ok(())
}

pub async fn get_signal(modem_id: &str) -> Result<MmcliSignalRoot> {
    let signal_output =
        run_cmd("mmcli", &["-m", modem_id, "--signal-get", "--output-json"]).await?;

    // TODO: get signal info based on current tech
    let signal: MmcliSignalRoot = serde_json::from_str(&signal_output)?;

    Ok(signal)
}

pub async fn get_location(modem_id: &str) -> Result<MmcliLocationRoot> {
    let location_output = run_cmd(
        "mmcli",
        &["-m", modem_id, "--location-get", "--output-json"],
    )
    .await?;

    let location = serde_json::from_str(&location_output)?;

    Ok(location)
}
