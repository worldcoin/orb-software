pub mod args;
pub mod settings;
pub mod orb_info;

use orb_build_info::{make_build_info, BuildInfo};

pub const BUILD_INFO: BuildInfo = make_build_info!();
