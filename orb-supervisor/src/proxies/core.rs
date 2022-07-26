//! Dbus proxies for interacting with orb core.

use zbus::dbus_proxy;

pub const SIGNUP_PROXY_DEFAULT_WELL_KNOWN_NAME: &str = "org.worldcoin.OrbCore1";
pub const SIGNUP_PROXY_DEFAULT_OBJECT_PATH: &str = "/org/worldcoin/OrbCore1/Signup";

#[dbus_proxy(interface = "org.worldcoin.OrbCore1.Signup")]
pub trait Signup {
    #[dbus_proxy(signal)]
    fn signup_started(&self) -> Result<()>;
}
