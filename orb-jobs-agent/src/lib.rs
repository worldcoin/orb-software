pub mod args;
mod connd;
pub mod handlers;
pub mod job_system;
pub mod program;
pub mod reboot;
pub mod settings;
pub mod shell;

use orb_build_info::{make_build_info, BuildInfo};

pub const BUILD_INFO: BuildInfo = make_build_info!();

/// Log target for job execution related logs.
/// Usage: `info!(target: ORB_JOB, "message")`
pub const ORB_JOB: &str = "orb_job";
