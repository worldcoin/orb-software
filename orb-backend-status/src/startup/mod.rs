#[cfg(all(feature = "android-collectors", not(feature = "linux-collectors")))]
pub mod android;
#[cfg(feature = "linux-collectors")]
pub mod linux;

use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;

pub struct Config {
    pub orb_id: OrbId,
    pub orb_name: OrbName,
    pub orb_jabil_id: OrbJabilId,
    pub orb_os_version: String,
    pub endpoint: Url,
    pub zenoh: zenorb::zenoh::Config,
    #[cfg(feature = "linux-collectors")]
    pub collectors: orb_backend_status::collectors::Config,
    pub metrics_socket: Option<String>,
}
