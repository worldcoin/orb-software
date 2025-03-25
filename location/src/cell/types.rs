//! Type definitions to encapsulate EC25 / EC21 AT command responses
//!
//! See https://files.pine64.org/doc/datasheet/project_anakin/LTE_module/Quectel_EC25&EC21_QuecCell_AT_Commands_Manual_V1.1.pdf

use eyre::{eyre, Result};

pub fn parse_opt_i32(field: &str) -> Option<i32> {
    match field.trim() {
        "-" | "" => None,
        s => s.parse().ok(),
    }
}

pub fn parse_opt_u32(field: &str) -> Option<u32> {
    match field.trim() {
        "-" | "" => None,
        s => s.parse().ok(),
    }
}

#[derive(Debug)]
pub enum Ec25ServingCell {
    Searching {
        state: String,
    },
    Gsm(GsmServing),
    Wcdma(WcdmaServing),
    Lte(LteServing),
    Unknown {
        state: String,
        rat: String,
        raw_fields: Vec<String>,
    },
}

#[derive(Debug)]
pub struct GsmServing {
    pub state: String,
    pub mcc: Option<u32>,
    pub mnc: Option<u32>,
    pub lac: String,
    pub cell_id: String,
    pub bsic: Option<u32>,
    pub arfcn: Option<u32>,
    pub band: String,
    pub rxlev: Option<i32>,
}

#[derive(Debug)]
pub struct WcdmaServing {
    pub state: String,
    pub mcc: Option<u32>,
    pub mnc: Option<u32>,
    pub lac: String,
    pub cell_id: String,
    pub uarfcn: Option<u32>,
    pub psc: Option<u32>,
    pub rscp: Option<i32>,
    pub ecio: Option<i32>,
}

#[derive(Debug)]
pub struct LteServing {
    pub state: String,
    pub is_tdd: Option<String>,
    pub mcc: Option<u32>,
    pub mnc: Option<u32>,
    pub cell_id: String,
    pub pcid: Option<u32>,
    pub earfcn: Option<u32>,
    pub freq_band_ind: Option<u32>,
    pub ul_bandwidth: Option<u32>,
    pub dl_bandwidth: Option<u32>,
    pub tac: String,
    pub rsrp: Option<i32>,
    pub rsrq: Option<i32>,
    pub rssi: Option<i32>,
    pub sinr: Option<i32>,
}

pub fn parse_ec25_serving_cell(fields: &[String]) -> Result<Ec25ServingCell> {
    if fields.len() == 1 {
        return Ok(Ec25ServingCell::Searching {
            state: fields[0].clone(),
        });
    }

    let state = fields[0].trim().to_string();
    let rat = fields[1].trim().to_string();

    match rat.as_str() {
        "GSM" => parse_gsm_serving(state, &fields[2..]).map(Ec25ServingCell::Gsm),
        "WCDMA" => parse_wcdma_serving(state, &fields[2..]).map(Ec25ServingCell::Wcdma),
        "LTE" => parse_lte_serving(state, &fields[2..]).map(Ec25ServingCell::Lte),
        _ => Ok(Ec25ServingCell::Unknown {
            state,
            rat,
            raw_fields: fields[2..].to_vec(),
        }),
    }
}

fn parse_gsm_serving(state: String, fields: &[String]) -> Result<GsmServing> {
    if fields.len() < 8 {
        return Err(eyre!(
            "Invalid GSM serving cell format. Fields: {:?}",
            fields
        ));
    }
    let mcc = parse_opt_u32(&fields[0]);
    let mnc = parse_opt_u32(&fields[1]);
    let lac = fields[2].clone();
    let cell_id = fields[3].clone();
    let bsic = parse_opt_u32(&fields[4]);
    let arfcn = parse_opt_u32(&fields[5]);
    let band = fields[6].clone();
    let rxlev = fields[7].parse().ok();

    Ok(GsmServing {
        state,
        mcc,
        mnc,
        lac,
        cell_id,
        bsic,
        arfcn,
        band,
        rxlev,
    })
}

fn parse_wcdma_serving(state: String, fields: &[String]) -> Result<WcdmaServing> {
    if fields.len() < 6 {
        return Err(eyre!("Invalid WCDMA serving cell format: {:?}", fields));
    }
    let mcc = parse_opt_u32(&fields[0]);
    let mnc = parse_opt_u32(&fields[1]);
    let lac = fields[2].clone();
    let cell_id = fields[3].clone();
    let uarfcn = parse_opt_u32(&fields[4]);
    let psc = parse_opt_u32(&fields[5]);
    let rscp = fields.get(6).and_then(|f| parse_opt_i32(f));
    let ecio = fields.get(7).and_then(|f| parse_opt_i32(f));

    Ok(WcdmaServing {
        state,
        mcc,
        mnc,
        lac,
        cell_id,
        uarfcn,
        psc,
        rscp,
        ecio,
    })
}

fn parse_lte_serving(state: String, fields: &[String]) -> Result<LteServing> {
    if fields.len() < 14 {
        return Err(eyre!("Invalid LTE serving cell format: {:?}", fields));
    }
    let is_tdd = {
        let x = fields[0].trim();
        if x == "-" || x.is_empty() {
            None
        } else {
            Some(x.to_string())
        }
    };
    let mcc = parse_opt_u32(&fields[1]);
    let mnc = parse_opt_u32(&fields[2]);
    let cell_id = fields[3].clone();
    let earfcn = parse_opt_u32(&fields[4]);
    let pcid = parse_opt_u32(&fields[5]);
    let freq_band_ind = parse_opt_u32(&fields[6]);
    let ul_bandwidth = parse_opt_u32(&fields[7]);
    let dl_bandwidth = None;
    let tac = fields[8].clone();
    let rsrp = parse_opt_i32(&fields[9]);
    let rsrq = parse_opt_i32(&fields[10]);
    let rssi = parse_opt_i32(&fields[11]);
    let sinr = parse_opt_i32(&fields[12]);

    Ok(LteServing {
        state,
        is_tdd,
        mcc,
        mnc,
        cell_id,
        pcid,
        earfcn,
        freq_band_ind,
        ul_bandwidth,
        dl_bandwidth,
        tac,
        rsrp,
        rsrq,
        rssi,
        sinr,
    })
}
