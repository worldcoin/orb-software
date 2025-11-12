//! Dbus proxies for interacting with orb core.

use zbus::proxy;

pub const SIGNUP_PROXY_DEFAULT_WELL_KNOWN_NAME: &str = "org.worldcoin.OrbCore1";
pub const SIGNUP_PROXY_DEFAULT_OBJECT_PATH: &str = "/org/worldcoin/OrbCore1/Signup";

#[proxy(
    interface = "org.worldcoin.OrbCore1.Signup",
    gen_blocking = false,
    default_service = "org.worldcoin.OrbCore1",
    default_path = "/org/worldcoin/OrbCore1/Signup"
)]
pub trait Signup {
    #[zbus(signal)]
    fn signup_started(&self) -> Result<()>;

    #[zbus(signal)]
    fn signup_finished(&self, success: bool) -> Result<()>;

    #[zbus(signal)]
    fn signup_ready(&self) -> Result<()>;

    #[zbus(signal)]
    fn signup_not_ready(&self, reason: &str) -> Result<()>;
}
