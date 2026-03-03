use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::Result;
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

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
/// wifi_add {"ssid":"HomeWIFI","sec":"Wpa2Psk","pwd":"12345678","hidden":"false","join_now":"false"}
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let wifi: WifiAdd = ctx.args_json()?;

    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;
    connd
        .add_wifi_profile(wifi.ssid.clone(), wifi.sec, wifi.pwd, wifi.hidden)
        .await?;

    let connect_result = if wifi.join_now {
        let network = connd.connect_to_wifi(wifi.ssid.clone()).await;
        Some(network)
    } else {
        None
    };

    let response = match connect_result {
        None => json!({ "connection_success": null, "network": null }),
        Some(Ok(network)) => json!({ "connection_success": true, "network": network }),
        Some(Err(e)) => {
            warn!(
                "failed to connect to network {}, removing it from saved profiles",
                wifi.ssid
            );

            // if we fail to connect, delete the profile
            // not the best place for this but it is what it is for now -vmenge
            if let Err(e) = connd.remove_wifi_profile(wifi.ssid.clone()).await {
                warn!(
                "failed to remove ssid {} after failed connection attempt. err: {e}",
                wifi.ssid
                );
            }

            json!({ "connection_success": false, "error": e.to_string() })
        }
    };

    Ok(ctx.success().stdout(response.to_string()))
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct WifiAdd {
    ssid: String,
    sec: String,
    pwd: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    join_now: bool,
}
