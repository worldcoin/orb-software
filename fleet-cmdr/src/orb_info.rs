use std::str::FromStr;

use color_eyre::eyre::Context;
use orb_endpoints::OrbId;

pub async fn get_orb_id() -> color_eyre::Result<OrbId> {
    let orb_id = if let Ok(orb_id) = std::env::var("ORB_ID") {
        assert!(!orb_id.is_empty());
        OrbId::from_str(orb_id.as_str())
            .wrap_err("Failed to parse ORB_ID from environment variable")
    } else {
        let output = tokio::process::Command::new("orb-id")
            .output()
            .await
            .wrap_err("failed to call orb-id binary")?;
        assert!(output.status.success(), "orb-id binary failed");
        String::from_utf8(output.stdout)
            .wrap_err("orb-id output was not utf8")
            .and_then(|orb_id| {
                OrbId::from_str(orb_id.as_str())
                    .wrap_err("Failed to parse orb-id output")
            })
    };

    orb_id
}

pub async fn get_orb_token() -> color_eyre::Result<String> {
    let token = std::env::var("ORB_TOKEN").unwrap_or_else(|_| "".to_string());
    Ok(token)
}
