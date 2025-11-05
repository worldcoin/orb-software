use super::{
    connection_state::ConnectionState, Location, Modem, ModemId, ModemInfo, Signal,
    SimId, SimInfo,
};
use crate::{modem_manager::ModemManager, utils::run_cmd};
use async_trait::async_trait;
use color_eyre::{
    eyre::{bail, ContextCompat},
    Result,
};
use regex::Regex;
use std::sync::LazyLock;

pub struct ModemManagerCli;

#[async_trait]
impl ModemManager for ModemManagerCli {
    async fn list_modems(&self) -> color_eyre::eyre::Result<Vec<Modem>> {
        let output = run_cmd("mmcli", &["-L"]).await?;
        Ok(parse_mmcli_modem_list(&output))
    }

    async fn modem_info(
        &self,
        modem_id: &ModemId,
    ) -> color_eyre::eyre::Result<super::ModemInfo> {
        let output = run_cmd("mmcli", &["-m", modem_id.as_str(), "-J"]).await?;
        parse_modem_info(&output)
    }

    async fn signal_setup(
        &self,
        modem_id: &ModemId,
        rate: std::time::Duration,
    ) -> color_eyre::eyre::Result<()> {
        run_cmd(
            "mmcli",
            &[
                "-m",
                modem_id.as_str(),
                "--signal-setup",
                &rate.as_secs().to_string(),
            ],
        )
        .await?;

        Ok(())
    }

    async fn signal_get(
        &self,
        modem_id: &ModemId,
    ) -> color_eyre::eyre::Result<super::Signal> {
        let output =
            run_cmd("mmcli", &["-m", modem_id.as_str(), "--signal-get", "-J"]).await?;

        parse_signal(&output)
    }

    async fn location_get(
        &self,
        modem_id: &ModemId,
    ) -> color_eyre::eyre::Result<super::Location> {
        let output = run_cmd(
            "mmcli",
            &["-m", modem_id.as_str(), "--location-get", "--output-json"],
        )
        .await?;

        parse_location(&output)
    }

    async fn sim_info(
        &self,
        sim_id: &SimId,
    ) -> color_eyre::eyre::Result<super::SimInfo> {
        let output = run_cmd("mmcli", &["-i", sim_id.as_str(), "-J"]).await?;

        parse_sim_info(&output)
    }

    async fn set_current_bands<'a>(
        &self,
        modem_id: &ModemId,
        bands: &[&'a str],
    ) -> Result<()> {
        if bands.is_empty() {
            bail!("bands arg cannot be empty!");
        }

        let bands = bands.join("|");
        let bands = format!("--set-current-bands={bands}");

        run_cmd("mmcli", &["-m", modem_id.as_str(), bands.as_str()]).await?;

        Ok(())
    }

    async fn set_allowed_and_preferred_modes<'a>(
        &self,
        modem_id: &ModemId,
        allowed: &[&'a str],
        preferred: &'a str,
    ) -> Result<()> {
        if allowed.is_empty() || preferred.is_empty() {
            bail!("bands and preferred args cannot be empty!");
        }

        let allowed = allowed.join("|");
        let allowed = format!("--set-allowed-modes={allowed}");

        let preferred = format!("--set-preferred-mode={preferred}");

        run_cmd(
            "mmcli",
            &[
                "-m",
                modem_id.as_str(),
                allowed.as_str(),
                preferred.as_str(),
            ],
        )
        .await?;

        Ok(())
    }
}

macro_rules! jerr {
    ($s:expr) => {
        format!("could not get {}", $s)
    };
}

fn parse_mmcli_modem_list(str: &str) -> Vec<Modem> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^/org/freedesktop/ModemManager\d+/Modem/(?P<id>\d+)\s+\[(?P<vendor>[^\]]+)]\s+(?P<model>.+)$").unwrap()
    });

    str.trim()
        .split("\n")
        .map(|line| line.trim())
        .filter(|line| line.starts_with("/org/freedesktop"))
        .filter_map(|line| {
            let captures = RE.captures(line.trim())?;
            let id = captures.name("id")?.as_str().parse().ok()?;
            let vendor = captures.name("vendor")?.as_str().to_string();
            let model = captures.name("model")?.as_str().to_string();

            Some(Modem { id, vendor, model })
        })
        .collect()
}

fn parse_modem_info(str: &str) -> Result<ModemInfo> {
    let json: serde_json::Value = serde_json::from_str(str)?;
    let state = json["modem"]["generic"]["state"]
        .as_str()
        .wrap_err(jerr!("modem.generic.state"))?;

    let state = ConnectionState::from(state.trim());

    let imei = json["modem"]["3gpp"]["imei"]
        .as_str()
        .wrap_err(jerr!("modem.3gpp.imei"))?
        .to_string();

    let operator_code = json["modem"]["3gpp"]["operator-code"]
        .as_str()
        .map(|oc| oc.to_string());

    let operator_name = json["modem"]["3gpp"]["operator-name"]
        .as_str()
        .map(|on| on.to_string());

    let access_tech = json["modem"]["generic"]["access-technologies"]
        .as_array()
        .and_then(|a| a.first()?.as_str())
        .map(|at| at.to_string());

    let sim = json["modem"]["generic"]["sim"]
        .as_str()
        .and_then(|s| s.split("/").last()?.parse().ok());

    Ok(ModemInfo {
        imei,
        operator_code,
        operator_name,
        access_tech,
        state,
        sim,
    })
}

fn parse_signal(str: &str) -> Result<Signal> {
    let json: serde_json::Value = serde_json::from_str(str)?;

    let get = |field: &str| -> Option<f64> {
        ["5g", "lte", "gsm", "umts", "cdma1x", "evdo"]
            .iter()
            .find_map(|access_tech| {
                json["modem"]["signal"][access_tech][field]
                    .as_str()
                    .and_then(|x| x.parse().ok())
            })
    };

    Ok(Signal {
        rsrp: get("rsrp"),
        rsrq: get("rsrq"),
        rssi: get("rssi"),
        snr: get("snr").or_else(|| get("sinr")),
    })
}

fn parse_sim_info(str: &str) -> Result<SimInfo> {
    let json: serde_json::Value = serde_json::from_str(str)?;

    let iccid = json["sim"]["properties"]["iccid"]
        .as_str()
        .wrap_err(jerr!("sim.properties.iccid"))?
        .to_string();

    let imsi = json["sim"]["properties"]["imsi"]
        .as_str()
        .wrap_err(jerr!("sim.properties.iccid"))?
        .to_string();

    Ok(SimInfo { iccid, imsi })
}

fn parse_location(str: &str) -> Result<Location> {
    let mut json: serde_json::Value = serde_json::from_str(str)?;
    let json = json["modem"]["location"]["3gpp"].take();
    let loc = serde_json::from_value(json)?;

    Ok(loc)
}

#[cfg(test)]
mod tests {
    use crate::modem_manager::{
        cli::{parse_location, parse_mmcli_modem_list, parse_signal},
        connection_state::ConnectionState,
        Location, Modem, ModemId, ModemInfo, Signal, SimId, SimInfo,
    };

    use super::{parse_modem_info, parse_sim_info};

    #[test]
    fn it_parses_modem_list() {
        // Arrange
        let val1 = "Found 1 modems:
        /org/freedesktop/ModemManager1/Modem/0 [Telit] LE910C4-WWXD
        /org/freedesktop/ModemManager1/Modem/2 [Sierra Wireless, Incorporated] MC8705";

        let val2 =
            "        /org/freedesktop/ModemManager1/Modem/9 [Telit] LE910C4-WWXD";

        let val3 = "";

        // Act
        let actual1 = parse_mmcli_modem_list(val1);
        let actual2 = parse_mmcli_modem_list(val2);
        let actual3 = parse_mmcli_modem_list(val3);

        // Assert
        let expected1 = vec![
            Modem {
                id: ModemId::from(0),
                vendor: "Telit".to_string(),
                model: "LE910C4-WWXD".to_string(),
            },
            Modem {
                id: ModemId::from(2),
                vendor: "Sierra Wireless, Incorporated".to_string(),
                model: "MC8705".to_string(),
            },
        ];

        let expected2 = vec![Modem {
            id: ModemId::from(9),
            vendor: "Telit".to_string(),
            model: "LE910C4-WWXD".to_string(),
        }];

        let expected3 = vec![];

        assert_eq!(actual1, expected1);
        assert_eq!(actual2, expected2);
        assert_eq!(actual3, expected3);
    }

    #[test]
    fn it_parses_modem_info() {
        let val = r#"{"modem":{"3gpp":{"5gnr":{"registration-settings":{"drx-cycle":"--","mico-mode":"--"}},"enabled-locks":["fixed-dialing"],"eps":{"initial-bearer":{"dbus-path":"/org/freedesktop/ModemManager1/Bearer/0","settings":{"apn":"em","ip-type":"ipv4","password":"--","user":"--"}},"ue-mode-operation":"csps-2"},"imei":"353338976168895","operator-code":"26202","operator-name":"vodafone.de","packet-service-state":"attached","pco":"--","registration-state":"roaming"},"cdma":{"activation-state":"--","cdma1x-registration-state":"--","esn":"--","evdo-registration-state":"--","meid":"--","nid":"--","sid":"--"},"dbus-path":"/org/freedesktop/ModemManager1/Modem/0","generic":{"access-technologies":["lte"],"bearers":["/org/freedesktop/ModemManager1/Bearer/1"],"carrier-configuration":"default","carrier-configuration-revision":"--","current-bands":["egsm","dcs","pcs","g850","utran-1","utran-4","utran-6","utran-5","utran-8","utran-2","eutran-1","eutran-2","eutran-3","eutran-4","eutran-5","eutran-7","eutran-8","eutran-9","eutran-12","eutran-13","eutran-14","eutran-18","eutran-19","eutran-20","eutran-25","eutran-26","eutran-28","utran-19"],"current-capabilities":["gsm-umts, lte"],"current-modes":"allowed: 2g, 3g, 4g; preferred: 4g","device":"/sys/devices/platform/bus@0/3610000.usb/usb1/1-2","device-identifier":"e867aad23fc10dc614959ad025a9d1049b3ad213","drivers":["option","qmi_wwan"],"equipment-identifier":"353338976168895","hardware-revision":"1.20","manufacturer":"Telit","model":"LE910C4-WWXD","own-numbers":[],"plugin":"telit","ports":["cdc-wdm0 (qmi)","ttyUSB0 (ignored)","ttyUSB1 (at)","ttyUSB2 (at)","wwan0 (net)"],"power-state":"on","primary-port":"cdc-wdm0","primary-sim-slot":"1","revision":"25.30.608  1  [Nov 14 2023 07:00:00]","signal-quality":{"recent":"yes","value":"75"},"sim":"/org/freedesktop/ModemManager1/SIM/0","sim-slots":["/org/freedesktop/ModemManager1/SIM/0","/"],"state":"connected","state-failed-reason":"--","supported-bands":["egsm","dcs","pcs","g850","utran-1","utran-4","utran-6","utran-5","utran-8","utran-2","eutran-1","eutran-2","eutran-3","eutran-4","eutran-5","eutran-7","eutran-8","eutran-9","eutran-12","eutran-13","eutran-14","eutran-18","eutran-19","eutran-20","eutran-25","eutran-26","eutran-28","utran-19"],"supported-capabilities":["gsm-umts, lte"],"supported-ip-families":["ipv4","ipv6","ipv4v6"],"supported-modes":["allowed: 2g; preferred: none","allowed: 3g; preferred: none","allowed: 4g; preferred: none","allowed: 2g, 3g; preferred: 3g","allowed: 2g, 3g; preferred: 2g","allowed: 2g, 4g; preferred: 4g","allowed: 2g, 4g; preferred: 2g","allowed: 3g, 4g; preferred: 4g","allowed: 3g, 4g; preferred: 3g","allowed: 2g, 3g, 4g; preferred: 4g","allowed: 2g, 3g, 4g; preferred: 3g","allowed: 2g, 3g, 4g; preferred: 2g"],"unlock-required":"sim-pin2","unlock-retries":["sim-pin (3)","sim-puk (10)","sim-pin2 (3)","sim-puk2 (10)"]}}}"#;

        let actual = parse_modem_info(val).unwrap();

        let expected = ModemInfo {
            imei: "353338976168895".to_string(),
            operator_code: Some("26202".to_string()),
            operator_name: Some("vodafone.de".to_string()),
            access_tech: Some("lte".to_string()),
            state: ConnectionState::Connected,
            sim: Some(SimId::from(0)),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn it_parses_signal() {
        let val = r#"{"modem":{"signal":{"5g":{"error-rate":"--","rsrp":"--","rsrq":"-14.00","snr":"--"},"cdma1x":{"ecio":"--","error-rate":"--","rssi":"-69.00"},"evdo":{"ecio":"--","error-rate":"--","io":"--","rssi":"--","sinr":"--"},"gsm":{"error-rate":"--","rssi":"-69.0"},"lte":{"error-rate":"--","rsrp":"-104.00","rsrq":"--","rssi":"--","snr":"2.00"},"refresh":{"rate":"10"},"threshold":{"error-rate":"no","rssi":"0"},"umts":{"ecio":"--","error-rate":"--","rscp":"--","rssi":"--"}}}}"#;

        let actual = parse_signal(val).unwrap();

        let expected = Signal {
            rsrp: Some(-104.0),
            rsrq: Some(-14.0),
            rssi: Some(-69.0),
            snr: Some(2.0),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn it_parses_sim_info() {
        let val = r#"{"sim":{"dbus-path":"/org/freedesktop/ModemManager1/SIM/0","properties":{"active":"yes","eid":"--","emergency-numbers":[],"esim-status":"--","gid1":"FFFFFFFFFF","gid2":"FFFFFFFFFF","iccid":"89883030000111825060","imsi":"295050905643977","operator-code":"29505","operator-name":"FL1","removability":"--","sim-type":"--"}}}"#;

        let actual = parse_sim_info(val).unwrap();

        let expected = SimInfo {
            iccid: "89883030000111825060".to_string(),
            imsi: "295050905643977".to_string(),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn it_parses_location() {
        let val = r#"{"modem":{"location":{"3gpp":{"cid":"0197763E","lac":"0000","mcc":"262","mnc":"03","tac":"00C945"},"cdma-bs":{"latitude":"--","longitude":"--"},"gps":{"altitude":"--","latitude":"--","longitude":"--","nmea":[],"utc":"--"}}}}"#;

        let actual = parse_location(val).unwrap();

        let expected = Location {
            cid: Some("0197763E".to_string()),
            lac: Some("0000".to_string()),
            mcc: Some("262".to_string()),
            mnc: Some("03".to_string()),
            tac: Some("00C945".to_string()),
        };

        assert_eq!(actual, expected);
    }
}
