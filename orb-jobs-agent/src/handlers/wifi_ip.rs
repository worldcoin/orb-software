use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::Context, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `wifi_ip`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    // Try to get the IP address of the currently connected WiFi interface
    // First, try to get the default route interface
    let route_output = ctx
        .deps()
        .shell
        .exec(&["ip", "route", "get", "8.8.8.8"])
        .await
        .wrap_err("failed to get default route")?
        .wait_with_output()
        .await
        .wrap_err("failed to get output for route command")?;

    let route_info = String::from_utf8_lossy(&route_output.stdout);

    // Extract the interface name from the route output
    let interface = route_info
        .split_whitespace()
        .skip_while(|&word| word != "dev")
        .nth(1)
        .unwrap_or("wlan0"); // fallback to common WiFi interface name

    // Get the IP address for the detected interface
    let ip_output = ctx
        .deps()
        .shell
        .exec(&["ip", "addr", "show", interface])
        .await
        .wrap_err_with(|| {
            format!("failed to get IP address for interface {interface}")
        })?
        .wait_with_output()
        .await
        .wrap_err("failed to get output for ip addr command")?;

    let ip_info = String::from_utf8_lossy(&ip_output.stdout);

    // Parse the IP address from the output
    let ip_address = ip_info
        .lines()
        .find(|line| line.trim().starts_with("inet ") && !line.contains("127.0.0.1"))
        .and_then(|line| {
            line.split_whitespace()
                .nth(1)
                .and_then(|addr| addr.split('/').next())
        })
        .unwrap_or("not found");

    let result = serde_json::json!({
        "interface": interface,
        "ip_address": ip_address,
        "status": if ip_address != "not found" { "connected" } else { "disconnected" }
    });

    Ok(ctx.success().stdout(result.to_string()))
}
