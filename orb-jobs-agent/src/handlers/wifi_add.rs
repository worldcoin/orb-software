use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result,
};
use orb_connd_dbus::ConndProxy;
use orb_relay_messages::jobs::v1::JobExecutionUpdate;

/// command format: `wifi_add <join_now> <ssid> <sec> <pwd> <hidden>`
/// example:
/// wifi_add true HomeWIFI wpa2 12345678 false
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    ensure!(
        ctx.args().len() == 5,
        "Expected 5 arguments, got {}",
        ctx.args().len()
    );

    let join_now: bool = ctx.args()[0].parse()?;
    let ssid = ctx.args()[1].to_owned();
    let sec = parse_sec(&ctx.args()[2])?.to_owned();

    let pwd = ctx.args()[3].to_owned();
    ensure!(pwd.len() >= 8, "Password: {pwd} is less than 8 characters");

    let hidden: bool = ctx.args()[4].parse()?;

    let connd = ConndProxy::new(&ctx.deps().session_dbus).await?;

    connd.add_wifi_profile(ssid, sec, pwd, hidden).await?;

    let joined_network = if join_now { true } else { false };

    let expected = json!({"profile_added": true, "joined_network": false});

    Ok(ctx.success())
}

fn parse_sec(s: &str) -> Result<&str> {
    match s.trim().to_lowercase().as_str() {
        "wpa3" => Ok("sae"),
        "wpa2" => Ok("wpa-psk"),
        _ => bail!("Invalid sec: {s}, allowed values are: ['wpa2', 'wpa3'] "),
    }
}
