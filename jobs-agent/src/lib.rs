pub mod args;
pub mod handlers;
pub mod job_system;
pub mod program;
pub mod settings;
pub mod shell;

use orb_build_info::{make_build_info, BuildInfo};

pub const BUILD_INFO: BuildInfo = make_build_info!();
