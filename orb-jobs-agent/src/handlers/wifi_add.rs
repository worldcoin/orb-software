use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::ensure, Result};
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// command format: `wifi_add <WifiAdd json>`
///
/// struct WifiAdd {
///     ssid: String,
///     sec: Sec,
///     pwd: String,
///     hidden: Option<bool>,
///     join_now: Option<bool>,
/// }
///
/// example:
/// wifi_add {"ssid":"HomeWIFI","sec":"wpa2","pwd":"12345678","hidden":"false","join_now":"false"}
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let wifi: WifiAdd = ctx.args_json()?;

    ensure!(wifi.pwd.len() >= 8, "Password should be at least 8 characters",);

    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    connd
        .add_wifi_profile(
            wifi.ssid.clone(),
            wifi.sec.as_str().into(),
            wifi.pwd,
            wifi.hidden,
        )
        .await?;

    let connection_success = if wifi.join_now {
        let connected = connd.connect_to_wifi(wifi.ssid).await.is_ok();
        Some(connected)
    } else {
        None
    };

    let response = json!({ "connection_success": connection_success }).to_string();

    Ok(ctx.success().stdout(response))
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct WifiAdd {
    ssid: String,
    sec: Sec,
    pwd: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    join_now: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy)]
#[serde(rename_all = "lowercase")]
enum Sec {
    Wpa2,
    Wpa3,
}

impl Sec {
    fn as_str(&self) -> &str {
        match self {
            Sec::Wpa2 => "wpa2",
            Sec::Wpa3 => "wpa3",
        }
    }
}
