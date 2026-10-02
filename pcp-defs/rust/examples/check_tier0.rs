//! Checks an unencrypted PCP tier0 exported by orb-core (`not-prod-pcp-export` +
//! `not-prod-pcp-no-encrypt`):
//! - every JSON and `.pb` file decodes into its `v1` type and re-encodes to the same
//!   bytes, so a key the protos lack cannot hide behind `ignore_unknown_fields`
//! - `hashes.json` is sorted and compact, and has a matching digest for every file
//! - given a second package, each JSON file has the same keys and value types
//!
//! Usage: `cargo run -p orb-pcp-defs --example check_tier0 -- <tier0.tar.gz> [other]`

use orb_pcp_defs::{
    prost::Message,
    v1::{
        BackendKeys, DiIrisEmbeddingShares, DiIrisEmbeddings, FaceEmbedding, Info,
        IrisCodeShares, IrisCodes,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error, fs::File, io::Read, process::ExitCode};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Files = BTreeMap<String, Vec<u8>>;

fn read_tier0(path: &str) -> Result<Files> {
    let mut archive =
        tar::Archive::new(flate2::read::GzDecoder::new(File::open(path)?));
    let mut files = Files::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        // tier0 is flat; this also accepts `./info.json` style entries.
        let Some(name) = entry
            .path()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            continue;
        };
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.insert(name, bytes);
    }
    Ok(files)
}

fn json_round_trip<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<bool> {
    let decoded: T = serde_json::from_slice(bytes)?;
    // `Value` sorts keys, matching how orb-core writes these files.
    Ok(serde_json::to_vec(&serde_json::to_value(&decoded)?)? == bytes)
}

fn pb_round_trip<T: Message + Default>(bytes: &[u8]) -> Result<bool> {
    Ok(T::decode(bytes)?.encode_to_vec() == bytes)
}

fn round_trip(name: &str, bytes: &[u8]) -> Result<bool> {
    match name {
        "info.json" => json_round_trip::<Info>(bytes),
        "iris_codes.json" => json_round_trip::<IrisCodes>(bytes),
        "backend_keys.json" => json_round_trip::<BackendKeys>(bytes),
        "face_embeddings.json" => json_round_trip::<Vec<FaceEmbedding>>(bytes),
        // Per-frame keys no message can name; checked as plain sorted JSON instead.
        "hashes.json" => json_round_trip::<Value>(bytes),
        "di_iris_embeddings.pb" => pb_round_trip::<DiIrisEmbeddings>(bytes),
        n if n.starts_with("iris_code_shares_") => {
            json_round_trip::<IrisCodeShares>(bytes)
        }
        n if n.starts_with("di_iris_embeddings_shares_") => {
            pb_round_trip::<DiIrisEmbeddingShares>(bytes)
        }
        n => Err(format!("no v1 type for {n}").into()),
    }
}

fn is_payload(name: &str) -> bool {
    name.ends_with(".json") || name.ends_with(".pb")
}

fn report(ok: bool, what: &str, name: &str) -> bool {
    println!("{}  {what:<24}{name}", if ok { "ok  " } else { "FAIL" });
    ok
}

fn check_package(files: &Files) -> Result<bool> {
    let mut all_ok = true;
    for (name, bytes) in files.iter().filter(|(n, _)| is_payload(n)) {
        all_ok &= match round_trip(name, bytes) {
            Ok(same) => report(same, "round trip", name),
            Err(e) => report(false, "decode", &format!("{name}: {e}")),
        };
    }
    let manifest: BTreeMap<String, String> =
        serde_json::from_slice(files.get("hashes.json").ok_or("missing hashes.json")?)?;
    // info.json is covered field by field: each salted value is hashed as value + salt.
    let unhashed = ["hashes.json", "info.json"];
    for (name, bytes) in files
        .iter()
        .filter(|(n, _)| is_payload(n) && !unhashed.contains(&n.as_str()))
    {
        let digest = hex::encode(Sha256::digest(bytes));
        all_ok &= report(
            manifest.get(name) == Some(&digest),
            "hashes.json entry",
            name,
        );
    }
    let info: BTreeMap<String, Value> =
        serde_json::from_slice(files.get("info.json").ok_or("missing info.json")?)?;
    for (field, salt) in info
        .iter()
        .filter_map(|(k, v)| Some((k.strip_suffix("_salt")?, v.as_str()?)))
    {
        let value = info.get(field).and_then(Value::as_str).unwrap_or_default();
        let digest = hex::encode(Sha256::digest(format!("{value}{salt}")));
        all_ok &= report(
            manifest.get(field) == Some(&digest),
            "hashes.json salted",
            field,
        );
    }
    Ok(all_ok)
}

/// Replaces values with their type; lengths vary per signup, so they are dropped.
fn shape(value: Value) -> Value {
    match value {
        Value::String(_) => "string".into(),
        Value::Number(_) => "number".into(),
        Value::Bool(_) => "bool".into(),
        Value::Array(a) => a.into_iter().map(shape).collect(),
        Value::Object(o) => o.into_iter().map(|(k, v)| (k, shape(v))).collect(),
        Value::Null => Value::Null,
    }
}

fn compare_shapes(a: &Files, b: &Files) -> Result<bool> {
    let mut all_ok = true;
    let names = a.keys().chain(b.keys()).filter(|n| n.ends_with(".json"));
    for name in names.collect::<std::collections::BTreeSet<_>>() {
        let same = match (a.get(name), b.get(name)) {
            (Some(x), Some(y)) => {
                shape(serde_json::from_slice(x)?) == shape(serde_json::from_slice(y)?)
            }
            _ => false,
        };
        all_ok &= report(same, "same shape", name);
    }
    Ok(all_ok)
}

fn run(paths: &[String]) -> Result<bool> {
    let packages = paths
        .iter()
        .map(|p| read_tier0(p))
        .collect::<Result<Vec<_>>>()?;
    let mut all_ok = true;
    for (path, files) in paths.iter().zip(&packages) {
        println!("== {path}");
        all_ok &= check_package(files)?;
    }
    if let [a, b] = packages.as_slice() {
        println!("== shape diff");
        all_ok &= compare_shapes(a, b)?;
    }
    Ok(all_ok)
}

fn main() -> ExitCode {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if !(1..=2).contains(&paths.len()) {
        eprintln!("usage: check_tier0 <tier0.tar.gz> [other-tier0.tar.gz]");
        return ExitCode::from(2);
    }
    match run(&paths) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
