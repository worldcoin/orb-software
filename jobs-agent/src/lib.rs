pub mod args;
pub mod handlers;
pub mod job_client;
pub mod orchestrator;

use orb_build_info::{make_build_info, BuildInfo};

pub const BUILD_INFO: BuildInfo = make_build_info!();
