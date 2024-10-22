use zbus::proxy;

#[proxy(
    default_service = "org.worldcoin.OrbSupervisor1",
    default_path = "/org/worldcoin/OrbSupervisor1/Manager",
    interface = "org.worldcoin.OrbSupervisor1.Manager"
)]
pub trait Supervisor {
    #[zbus(property)]
    fn background_downloads_allowed(&self) -> zbus::Result<bool>;

    #[zbus(name = "RequestUpdatePermission")]
    fn request_update_permission(&self) -> zbus::Result<()>;
}

#[proxy(
    default_service = "org.worldcoin.AuthTokenManager1",
    default_path = "/org/worldcoin/AuthTokenManager1",
    interface = "org.worldcoin.AuthTokenManager1"
)]
trait AuthToken {
    #[zbus(property)]
    fn token(&self) -> zbus::Result<String>;
}
