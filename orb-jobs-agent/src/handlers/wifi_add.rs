use crate::{
    connd,
    job_system::ctx::{Ctx, JobExecutionUpdateExt},
};
use color_eyre::Result;
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

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

    let (connection_success, network) = if wifi.join_now {
        let network = connd::connect_to_wifi_and_wait_for_internet(
            &connd,
            &wifi.ssid,
            Duration::from_secs(10),
        )
        .await
        .ok();

        ctx.force_relay_reconnect().await?;

        (Some(network.is_some()), network)
    } else {
        (None, None)
    };

    let response =
        json!({ "connection_success": connection_success, "network": network })
            .to_string();

    Ok(ctx.success().stdout(response))
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
