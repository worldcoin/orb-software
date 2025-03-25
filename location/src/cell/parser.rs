use crate::cell::data::{NeighborCell, ServingCell};
use crate::cell::types::{
    parse_ec25_serving_cell, parse_opt_i32, parse_opt_u32, Ec25ServingCell,
};
use eyre::{eyre, Result};

fn split_quoted_fields(line: &str) -> Vec<String> {
    line.split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect()
}

pub fn parse_serving_cell(response: &str) -> Result<ServingCell> {
    const SERVINGCELL_PREFIX: &str = "+QENG: \"servingcell\",";

    let lines: Vec<&str> = response
        .lines()
        .filter(|l| l.contains("+QENG: \"servingcell\""))
        .collect();

    if lines.is_empty() {
        return Err(eyre!("No +QENG: \"servingcell\" lines found in response."));
    }

    let mut final_parsed: Option<Ec25ServingCell> = None;

    for line in lines {
        if let Some(idx) = line.find(SERVINGCELL_PREFIX) {
            let after = &line[idx + SERVINGCELL_PREFIX.len()..];
            let raw_fields = split_quoted_fields(after);
            if !raw_fields.is_empty() {
                match parse_ec25_serving_cell(&raw_fields) {
                    Ok(parsed) => {
                        final_parsed =
                            Some(merge_prefer_new(final_parsed.take(), parsed));
                    }
                    Err(e) => {
                        return Err(eyre!(
                            "Failed to parse serving cell line '{}': {}",
                            line,
                            e
                        ));
                    }
                }
            }
        }
    }

    match final_parsed {
        Some(ec25) => Ok(ec25.into()),
        None => Err(eyre!("Could not parse any recognized serving cell lines.")),
    }
}

fn merge_prefer_new(
    old: Option<Ec25ServingCell>,
    new: Ec25ServingCell,
) -> Ec25ServingCell {
    use Ec25ServingCell::*;
    match (old, new) {
        (None, new_val) => new_val,
        (Some(_), new_val @ Lte(_)) => new_val,
        (Some(_), new_val @ Wcdma(_)) => new_val,
        (Some(_), new_val @ Gsm(_)) => new_val,
        (Some(old_val @ Lte(_)), Unknown { .. }) => old_val,
        (Some(old_val @ Wcdma(_)), Searching { .. }) => old_val,
        (Some(old_val @ Gsm(_)), Searching { .. }) => old_val,
        (Some(_), new_val) => new_val,
    }
}

impl From<Ec25ServingCell> for ServingCell {
    fn from(ec25: Ec25ServingCell) -> Self {
        use Ec25ServingCell::*;
        match ec25 {
            Searching { state } => ServingCell {
                connection_status: state,
                network_type: "UNKNOWN".to_string(),
                duplex_mode: "-".to_string(),
                mcc: None,
                mnc: None,
                cell_id: "-".to_string(),
                channel_or_arfcn: None,
                pcid_or_psc: None,
                rsrp: None,
                rsrq: None,
                rssi: None,
                sinr: None,
            },
            Unknown {
                state,
                rat,
                raw_fields,
            } => ServingCell {
                connection_status: state,
                network_type: rat,
                duplex_mode: "-".to_string(),
                mcc: None,
                mnc: None,
                cell_id: format!("Unknown fields: {:?}", raw_fields),
                channel_or_arfcn: None,
                pcid_or_psc: None,
                rsrp: None,
                rsrq: None,
                rssi: None,
                sinr: None,
            },
            Gsm(g) => ServingCell {
                connection_status: g.state,
                network_type: "GSM".to_string(),
                duplex_mode: "-".to_string(),
                mcc: g.mcc,
                mnc: g.mnc,
                cell_id: g.cell_id,
                channel_or_arfcn: g.arfcn,
                pcid_or_psc: None,
                rsrp: None,
                rsrq: None,
                rssi: g.rxlev,
                sinr: None,
            },
            Wcdma(w) => ServingCell {
                connection_status: w.state,
                network_type: "WCDMA".to_string(),
                duplex_mode: "-".to_string(),
                mcc: w.mcc,
                mnc: w.mnc,
                cell_id: w.cell_id,
                channel_or_arfcn: w.uarfcn,
                pcid_or_psc: w.psc,
                rsrp: w.rscp,
                rsrq: None,
                rssi: None,
                sinr: w.ecio,
            },
            Lte(l) => ServingCell {
                connection_status: l.state,
                network_type: "LTE".to_string(),
                duplex_mode: l.is_tdd.unwrap_or("-".to_string()),
                mcc: l.mcc,
                mnc: l.mnc,
                cell_id: l.cell_id,
                channel_or_arfcn: l.earfcn,
                pcid_or_psc: l.pcid,
                rsrp: l.rsrp,
                rsrq: l.rsrq,
                rssi: l.rssi,
                sinr: l.sinr,
            },
        }
    }
}

pub fn parse_neighbor_cells(response: &str) -> Result<Vec<NeighborCell>> {
    // Do not include the closing quote to appropriately capture
    // intra/inter neighbourcell lines.
    //
    // This is important as it's possible we could capture intra LTE
    // neighbourcell information (which should mean we're on the same
    // cell as the servingcell if we're on LTE) and seed a couple
    // additional cell-tower fields instead of the lone one we get ATM
    const NEIGHBOURCELL_PREFIX: &str = "+QENG: \"neighbourcell";

    let mut results = Vec::new();

    for line in response.lines() {
        if let Some(idx) = line.find(NEIGHBOURCELL_PREFIX) {
            let after = &line[idx + NEIGHBOURCELL_PREFIX.len()..];
            let after = if let Some(cidx) = after.find(',') {
                &after[cidx + 1..]
            } else {
                after // Should prolly bubble an error here
            };

            let fields = split_quoted_fields(after);
            if fields.is_empty() {
                continue;
            }
            let rat = fields[0].trim().to_string();
            if let Some(cell) = parse_neighbor_fields(&rat, &fields[1..]) {
                results.push(cell);
            }
        }
    }
    Ok(results)
}

/// Parse neighbourcell fields based on RAT
///
/// ## TODO
/// This should maybe normalize out hex fields into just numeric fields
/// or do better bounds checking...
fn parse_neighbor_fields(rat: &str, fields: &[String]) -> Option<NeighborCell> {
    match rat {
        "GSM" => {
            if fields.len() < 6 {
                return None;
            }
            let bsic = parse_opt_u32(&fields[4]);
            let arfcn = parse_opt_u32(&fields[5]);
            let rssi = fields.get(6).and_then(|f| parse_opt_i32(f));
            Some(NeighborCell {
                network_type: "GSM".to_string(),
                channel_or_arfcn: arfcn,
                pcid_or_psc: bsic,
                rsrp: None,
                rsrq: None,
                rssi,
                sinr: None,
            })
        }
        "WCDMA" => {
            if fields.len() < 2 {
                return None;
            }
            let uarfcn = parse_opt_u32(&fields[0]);
            let psc = parse_opt_u32(&fields[1]);
            let rscp = fields.get(2).and_then(|f| parse_opt_i32(f));
            let ecno = fields.get(3).and_then(|f| parse_opt_i32(f));
            Some(NeighborCell {
                network_type: "WCDMA".to_string(),
                channel_or_arfcn: uarfcn,
                pcid_or_psc: psc,
                rsrp: rscp,
                rsrq: None,
                rssi: None,
                sinr: ecno,
            })
        }
        "LTE" => {
            if fields.is_empty() {
                return None;
            }
            let earfcn = parse_opt_u32(&fields[0]);
            let pcid = fields.get(1).and_then(|f| parse_opt_u32(f));
            let rsrp = fields.get(2).and_then(|f| parse_opt_i32(f));
            let rsrq = fields.get(3).and_then(|f| parse_opt_i32(f));
            let rssi = fields.get(4).and_then(|f| parse_opt_i32(f));
            let sinr = fields.get(5).and_then(|f| parse_opt_i32(f));
            Some(NeighborCell {
                network_type: "LTE".to_string(),
                channel_or_arfcn: earfcn,
                pcid_or_psc: pcid,
                rsrp,
                rsrq,
                rssi,
                sinr,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_quoted_fields() {
        let line = "\"CONNECT\",\"LTE\",\"FDD\",310,260,\"1234\"";
        let fields = split_quoted_fields(line);
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0], "CONNECT");
        assert_eq!(fields[1], "LTE");
        assert_eq!(fields[2], "FDD");
        assert_eq!(fields[3], "310");
        assert_eq!(fields[4], "260");
        assert_eq!(fields[5], "1234");
    }

    #[test]
    fn test_parse_serving_cell_minimal_search() {
        let raw = r#"
            +QENG: "servingcell","SEARCH"
        "#;
        let parsed = parse_serving_cell(raw).unwrap();
        assert_eq!(parsed.connection_status, "SEARCH");
        assert_eq!(parsed.network_type, "UNKNOWN");
    }

    #[test]
    fn test_parse_serving_cell_gsm() {
        let raw = r#"
            +QENG: "servingcell","CONNECT","GSM",460,00,550A,2BB9,23,94,0,-61
        "#;
        let parsed = parse_serving_cell(raw).unwrap();
        assert_eq!(parsed.connection_status, "CONNECT");
        assert_eq!(parsed.network_type, "GSM");
        assert_eq!(parsed.mcc, Some(460));
        assert_eq!(parsed.mnc, Some(0));
        assert_eq!(parsed.channel_or_arfcn, Some(94));
        assert_eq!(parsed.rssi, Some(-61));
    }

    #[test]
    fn test_parse_serving_cell_lte() {
        let raw = r#"
            +QENG: "servingcell","NOCONN","LTE","FDD",310,260,"12345678",6300,150,0,0,"00AB",-95,-13,-70,25,99
        "#;
        let parsed = parse_serving_cell(raw).unwrap();
        assert_eq!(parsed.connection_status, "NOCONN");
        assert_eq!(parsed.network_type, "LTE");
        assert_eq!(parsed.duplex_mode, "FDD");
        assert_eq!(parsed.mcc, Some(310));
        assert_eq!(parsed.mnc, Some(260));
        assert_eq!(parsed.cell_id, "12345678");
        assert_eq!(parsed.channel_or_arfcn, Some(6300));
        assert_eq!(parsed.pcid_or_psc, Some(150));
        assert_eq!(parsed.rsrp, Some(-95));
        assert_eq!(parsed.rsrq, Some(-13));
        assert_eq!(parsed.rssi, Some(-70));
        assert_eq!(parsed.sinr, Some(25));
    }

    #[test]
    fn test_parse_neighbor_cells_gsm() {
        let raw = r#"
        +QENG: "neighbourcell","GSM",460,01,5504,2B55,52,123,0
        +QENG: "neighbourcell","GSM",99,100,101,102,103,104,105
    "#;
        let cells = parse_neighbor_cells(raw).unwrap();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].network_type, "GSM");
        assert_eq!(cells[0].channel_or_arfcn, Some(123));
        assert_eq!(cells[0].pcid_or_psc, Some(52));
        assert_eq!(cells[1].channel_or_arfcn, Some(104));
        assert_eq!(cells[1].pcid_or_psc, Some(103));
    }

    #[test]
    fn test_parse_neighbor_cells_lte() {
        let raw = r#"
        +QENG: "neighbourcell intra","LTE",38950,276,-3,-88,-65,0,37,7,16
        +QENG: "neighbourcell inter","LTE",39148,-,-,-,-,-,37,0,30,7
    "#;
        let cells = parse_neighbor_cells(raw).unwrap();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].network_type, "LTE");
        assert_eq!(cells[0].channel_or_arfcn, Some(38950));
        assert_eq!(cells[0].pcid_or_psc, Some(276));
        assert_eq!(cells[0].rsrp, Some(-3));
        assert_eq!(cells[0].rsrq, Some(-88));
        assert_eq!(cells[0].rssi, Some(-65));
        assert_eq!(cells[0].sinr, Some(0));
    }
}
