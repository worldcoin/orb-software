#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::ignored_unit_patterns,
    clippy::items_after_statements
)]

pub mod consts;
pub mod interfaces;
pub mod proxies;
pub mod shutdown;
pub mod startup;
pub mod tasks;

use orb_build_info::{make_build_info, BuildInfo};
use tokio::fs;

pub const BUILD_INFO: BuildInfo = make_build_info!();

pub enum Orb {
    Diamond,
    Pearl,
    Unknown,
}

impl Orb {
    pub async fn from_fs() -> Orb {
        let str = fs::read_to_string("/usr/persistent/hardware_version")
            .await
            .map_or_else(|_| String::new(), |str| str.trim().to_lowercase());

        if str.contains("diamond") {
            Orb::Diamond
        } else if str.contains("pearl") {
            Orb::Pearl
        } else {
            Orb::Unknown
        }
    }
}
