#[cfg(all(feature = "android-collectors", not(feature = "linux-collectors")))]
pub mod android;
#[cfg(feature = "linux-collectors")]
pub mod linux;

use orb_backend_status::collectors;
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;

pub struct Config {
    pub orb_id: OrbId,
    pub orb_name: OrbName,
    pub orb_jabil_id: OrbJabilId,
    pub orb_os_version: String,
    pub endpoint: Url,
    pub zenoh: zenorb::zenoh::Config,
    pub collectors: collectors::Config,
    pub metrics_socket: Option<String>,
}
