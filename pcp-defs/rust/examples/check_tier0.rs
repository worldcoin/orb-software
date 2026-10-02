//! Decodes every tier0 file of an unencrypted PCP into its `v1` type and checks
//! it re-encodes to the same bytes, so a field the protos miss cannot hide behind
//! `ignore_unknown_fields`. Usage: `cargo run -p orb-pcp-defs --example check_tier0 -- <tier0.tar.gz>`

use orb_pcp_defs::{
    prost::Message,
    v1::{
        BackendKeys, DiIrisEmbeddingShares, DiIrisEmbeddings, FaceEmbedding, Info,
        IrisCodeShares, IrisCodes,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use std::{error::Error, fs::File, io::Read, process::ExitCode};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn json_round_trip<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<bool> {
    let decoded: T = serde_json::from_slice(bytes)?;
    // `Value` sorts keys, matching how orb-core writes these files.
    Ok(serde_json::to_vec(&serde_json::to_value(&decoded)?)? == bytes)
}

fn pb_round_trip<T: Message + Default>(bytes: &[u8]) -> Result<bool> {
    Ok(T::decode(bytes)?.encode_to_vec() == bytes)
}

fn check(name: &str, bytes: &[u8]) -> Option<Result<bool>> {
    Some(match name {
        "info.json" => json_round_trip::<Info>(bytes),
        "iris_codes.json" => json_round_trip::<IrisCodes>(bytes),
        "backend_keys.json" => json_round_trip::<BackendKeys>(bytes),
        "face_embeddings.json" => json_round_trip::<Vec<FaceEmbedding>>(bytes),
        "di_iris_embeddings.pb" => pb_round_trip::<DiIrisEmbeddings>(bytes),
        n if n.starts_with("iris_code_shares_") => {
            json_round_trip::<IrisCodeShares>(bytes)
        }
        n if n.starts_with("di_iris_embeddings_shares_") => {
            pb_round_trip::<DiIrisEmbeddingShares>(bytes)
        }
        // hashes.json carries per-frame keys no message can name.
        _ => return None,
    })
}

fn run(path: &str) -> Result<bool> {
    let mut archive =
        tar::Archive::new(flate2::read::GzDecoder::new(File::open(path)?));
    let mut all_ok = true;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        match check(&name, &bytes) {
            None => continue,
            Some(Ok(true)) => println!("ok    round trip      {name}"),
            Some(Ok(false)) => {
                println!("FAIL  re-encode differs  {name}");
                all_ok = false;
            }
            Some(Err(e)) => {
                println!("FAIL  decode  {name}: {e}");
                all_ok = false;
            }
        }
    }
    Ok(all_ok)
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: check_tier0 <tier0.tar.gz>");
        return ExitCode::from(2);
    };
    match run(&path) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("reading {path}: {e}");
            ExitCode::FAILURE
        }
    }
}
