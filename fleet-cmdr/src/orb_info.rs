use std::str::FromStr;

use color_eyre::eyre::Context;
use orb_endpoints::OrbId;

pub async fn get_orb_id() -> color_eyre::Result<OrbId> {
    let output = tokio::process::Command::new("orb-id")
        .output()
        .await
        .wrap_err("failed to call orb-id binary")?;
    assert!(output.status.success(), "orb-id binary failed");
    String::from_utf8(output.stdout)
        .wrap_err("orb-id output was not utf8")
        .and_then(|orb_id| {
            OrbId::from_str(orb_id.as_str()).wrap_err("Failed to parse orb-id output")
        })
}

pub async fn get_orb_token() -> color_eyre::Result<String> {
    let output = tokio::process::Command::new("orb-token")
        .output()
        .await
        .wrap_err("failed to call orb-token binary")?;
    assert!(output.status.success(), "orb-token binary failed");
    String::from_utf8(output.stdout).wrap_err("orb-token output was not utf8")
}
