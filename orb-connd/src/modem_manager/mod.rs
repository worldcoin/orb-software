use crate::{
    telemetry::{
        connection_state::ConnectionState, location::MmcliLocationRoot,
        signal::MmcliSignalRoot,
    },
    utils::run_cmd,
};
use color_eyre::{
    eyre::{eyre, ContextCompat},
    Result,
};
use std::time::Duration;

// todo: refactor this into something nicer leveraging serde_json::Value
// too many unecessary calls to mmcli, we only need to get info once and then we can
// parse from it

pub async fn modem_info(modem_id: &str) -> Result<serde_json::Value> {
    let output = run_cmd("mmcli", &["-m", modem_id, "-J"]).await?;
    let modem_info = serde_json::from_str(&output)?;

    Ok(modem_info)
}

pub async fn get_sim_id(modem_id: &str) -> Result<usize> {
    let modem_info = modem_info(modem_id).await?;
    modem_info
        .get("modem")
        .and_then(|m| {
            m.get("generic")?
                .get("sim")?
                .as_str()?
                .split("/")
                .last()?
                .parse()
                .ok()
        })
        .wrap_err_with(|| {
            format!(
                "failed to get SIM for modem_id {modem_id}. modem_info: {modem_info}"
            )
        })
}

pub async fn bearer_info(bearer_id: usize) -> Result<serde_json::Value> {
    let output = run_cmd("mmcli", &["-b", &bearer_id.to_string(), "-J"]).await?;
    let modem_info = serde_json::from_str(&output)?;

    Ok(modem_info)
}

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

pub async fn get_iccid(sim_id: usize) -> Result<String> {
    let sim_output =
        run_cmd("mmcli", &["-i", &sim_id.to_string(), "--output-keyvalue"]).await?;

    let iccid = retrieve_value(&sim_output, "sim.properties.iccid")?;

    Ok(iccid)
}

pub async fn get_connection_state(modem_id: &str) -> Result<ConnectionState> {
    let output = run_cmd("mmcli", &["-m", modem_id, "-K"]).await?;
    let operator: String = retrieve_value(&output, "modem.generic.state")?;

    Ok(ConnectionState::from(operator))
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

/// has a 30s timeout by default
pub async fn simple_connect(modem_id: &str, timeout: Duration) -> Result<()> {
    let timeout = timeout.as_secs();

    run_cmd(
        "mmcli",
        &[
            "-m",
            modem_id,
            &format!("--timeout={timeout}"),
            "--simple-connect=apn=em,ip-type=ipv4",
        ],
    )
    .await?;

    Ok(())
}

pub async fn simple_disconnect(modem_id: &str) -> Result<()> {
    run_cmd("mmcli", &["-m", modem_id, "--simple-disconnect"]).await?;
    Ok(())
}

fn retrieve_value(output: &str, key: &str) -> Result<String> {
    output
        .lines()
        .find(|l| l.starts_with(key))
        .ok_or_else(|| eyre!("Key {key} not found"))?
        .split_once(':')
        .ok_or_else(|| eyre!("Malformed line for key {key}"))
        .map(|(_, v)| v.trim().to_string())
}
