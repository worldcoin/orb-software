use clap::Parser;
use eyre::{eyre, Result};
use orb_relay_client::debug_any;
use orb_relay_messages::prost_types::Any;
use serde_json::Value;

#[derive(Parser, Debug)]
struct Args {
    #[arg()]
    json: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    println!("{}", decode_payload(&args.json)?);
    Ok(())
}

fn decode_payload(json: &str) -> Result<String> {
    println!("json: {}", json);
    let v: Value = json5::from_str(json)?;
    let any = Any {
        type_url: v["type_url"]
            .as_str()
            .ok_or_else(|| eyre!("Invalid type_url"))?
            .to_string(),
        value: v["value"]
            .as_array()
            .ok_or_else(|| eyre!("Invalid value"))?
            .iter()
            .map(|n| {
                n.as_u64()
                    .ok_or_else(|| eyre!("Invalid number"))
                    .map(|n| n as u8)
            })
            .collect::<Result<_>>()?,
    };
    Ok(debug_any(&Some(any)))
}
