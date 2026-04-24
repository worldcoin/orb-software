use crate::{
    conn_change,
    handlers::{
        beacon, change_name, check_my_orb, fsck, gondor, logs, mcu, netconfig_get,
        netconfig_set, orb_details, read_file, read_gimbal, reboot, reset_gimbal,
        reset_rgb_focus_calibration, sec_mcu_reboot, service, slot_switch, speed_test,
        thermal_cam_recalibration, update_versions, wifi_add, wifi_connect, wifi_ip,
        wifi_list, wifi_remove, wifi_scan, wipe_downloads,
    },
    job_system::handler::JobHandler,
    settings::Settings,
    shell::Shell,
};
use color_eyre::Result;
use std::sync::Arc;
use tokio::fs;
use zenorb::Zenorb;

/// Dependencies used by the jobs-agent.
#[derive(Clone)]
pub struct Deps {
    pub shell: Arc<dyn Shell>,
    pub zenorb: Zenorb,
    pub session_dbus: zbus::Connection,
    pub settings: Settings,
}

impl Deps {
    pub fn new<S>(
        shell: S,
        session_dbus: zbus::Connection,
        zenorb: Zenorb,
        settings: Settings,
    ) -> Self
    where
        S: Shell + 'static,
    {
        Self {
            shell: Arc::new(shell),
            zenorb,
            session_dbus,
            settings,
        }
    }
}

pub async fn run(deps: Deps) -> Result<()> {
    fs::create_dir_all(&deps.settings.store_path).await?;

    let job_handler = JobHandler::builder()
        .parallel("read_file", read_file::handler)
        .parallel("beacon", beacon::handler)
        .parallel("change_name", change_name::handler)
        .parallel("check_my_orb", check_my_orb::handler)
        .parallel("fsck", fsck::handler)
        .parallel("gondor", gondor::handler)
        .parallel("orb_details", orb_details::handler)
        .parallel("read_gimbal", read_gimbal::handler)
        .parallel("reset_gimbal", reset_gimbal::handler)
        .parallel("mcu", mcu::handler)
        .parallel("sec_mcu_reboot", sec_mcu_reboot::handler)
        .parallel("wifi_ip", wifi_ip::handler)
        .parallel("wifi_add", wifi_add::handler)
        .parallel("wifi_connect", wifi_connect::handler)
        .parallel("wifi_remove", wifi_remove::handler)
        .parallel("wipe_downloads", wipe_downloads::handler)
        .parallel("wifi_list", wifi_list::handler)
        //        .parallel("wifi_scan", wifi_scan::handler)
        .parallel("netconfig_get", netconfig_get::handler)
        .parallel("netconfig_set", netconfig_set::handler)
        .parallel("service", service::handler)
        .parallel("speed_test", speed_test::handler)
        .sequential(
            "thermal_cam_recalibration",
            thermal_cam_recalibration::handler,
        )
        .sequential(
            "reset_rgb_focus_calibration",
            reset_rgb_focus_calibration::handler,
        )
        .sequential("update_versions", update_versions::handler)
        .parallel_max("logs", 3, logs::handler)
        .sequential("reboot", reboot::handler)
        .sequential("slot_switch", slot_switch::handler)
        .build(deps.clone());

    conn_change::spawn_watcher(&deps.zenorb, job_handler.job_client.clone()).await?;

    job_handler.run().await;

    Ok(())
}
