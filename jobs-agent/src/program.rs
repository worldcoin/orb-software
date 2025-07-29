use crate::{
    handlers::{check_my_orb, logs, mcu, orb_details, read_file, read_gimbal, reboot},
    job_system::handler::JobHandler,
    settings::Settings,
    shell::Shell,
};

#[derive(Debug)]
pub struct Deps {
    pub shell: Box<dyn Shell>,
    pub settings: Settings,
}

pub async fn run(deps: Deps) {
    JobHandler::builder()
        .parallel("read_file", read_file::handler)
        .parallel("check_my_orb", check_my_orb::handler)
        .parallel("orb_details", orb_details::handler)
        .parallel("read_gimbal", read_gimbal::handler)
        .parallel("mcu", mcu::handler)
        .parallel_max("logs", 3, logs::handler)
        .sequential("reboot", reboot::handler)
        .build(deps)
        .run()
        .await;
}
