pub mod args;
pub mod handlers;
pub mod orb_info;
pub mod settings;

use orb_build_info::{make_build_info, BuildInfo};

pub const BUILD_INFO: BuildInfo = make_build_info!();
