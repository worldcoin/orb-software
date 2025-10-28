use crate::{
    handlers::{
        beacon, check_my_orb, logs, mcu, orb_details, read_file, read_gimbal, reboot,
        reset_gimbal, sec_mcu_reboot, wifi_ip,
    },
    job_system::handler::JobHandler,
    settings::Settings,
    shell::Shell,
};
use color_eyre::Result;
use tokio::fs;

/// Dependencies used by the jobs-agent.
#[derive(Debug)]
pub struct Deps {
    pub shell: Box<dyn Shell>,
    pub settings: Settings,
}

impl Deps {
    pub fn new<S>(shell: S, settings: Settings) -> Self
    where
        S: Shell + 'static,
    {
        Self {
            shell: Box::new(shell),
            settings,
        }
    }
}

pub async fn run(deps: Deps) -> Result<()> {
    fs::create_dir_all(&deps.settings.store_path).await?;

    JobHandler::builder()
        .parallel("read_file", read_file::handler)
        .parallel("beacon", beacon::handler)
        .parallel("check_my_orb", check_my_orb::handler)
        .parallel("orb_details", orb_details::handler)
        .parallel("read_gimbal", read_gimbal::handler)
        .parallel("reset_gimbal", reset_gimbal::handler)
        .parallel("mcu", mcu::handler)
        .parallel("wifi_ip", wifi_ip::handler)
        .parallel("sec_mcu_reboot", sec_mcu_reboot::handler)
        .parallel_max("logs", 3, logs::handler)
        .sequential("reboot", reboot::handler)
        .build(deps)
        .run()
        .await;

    Ok(())
}
