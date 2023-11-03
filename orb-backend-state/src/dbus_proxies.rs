//! Proxy objects for other dbus interfaces.

use zbus::dbus_proxy;

/// AuthToken is a DBus interface that exposes currently valid backend token via
/// 'token' property.
///
/// When token is refreshed, the property is updated and a signal is emitted.
#[dbus_proxy(
    default_service = "org.worldcoin.AuthTokenManager1",
    default_path = "/org/worldcoin/AuthTokenManager1",
    interface = "org.worldcoin.AuthTokenManager1"
)]
trait AuthToken {
    #[dbus_proxy(property)]
    fn token(&self) -> zbus::Result<String>;
}
