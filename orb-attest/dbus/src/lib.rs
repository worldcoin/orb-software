//! Get current value of token
//! gdbus call --session -d org.worldcoin.AuthTokenManager1 -o '/org/worldcoin/AuthTokenManager1' -m
//! org.freedesktop.DBus.Properties.Get org.worldcoin.AuthTokenManager1 Token
//!
//! Force token refresh
//! gdbus call --session -d org.worldcoin.AuthTokenManager1 -o '/org/worldcoin/AuthTokenManager1' -m
//! org.worldcoin.AuthTokenManager1.ForceTokenRefresh
//!
//! Wait for token refresh
//! dbus-monitor type='signal',sender='org.worldcoin.AuthTokenManager1'

use zbus::interface;

pub trait AuthTokenManagerT: Send + Sync + 'static {
    fn token(&self) -> zbus::fdo::Result<String>;
    fn force_token_refresh(&mut self, ctxt: zbus::SignalContext<'_>);
}

#[derive(Debug, derive_more::From)]
pub struct AuthTokenManager<T>(pub T);

#[interface(
    name = "org.worldcoin.AuthTokenManager1",
    proxy(
        default_service = "org.worldcoin.AuthTokenManager1",
        default_path = "/org/worldcoin/AuthTokenManager1",
    )
)]
impl<T: AuthTokenManagerT> AuthTokenManagerT for AuthTokenManager<T> {
    #[zbus(property)]
    fn token(&self) -> zbus::fdo::Result<String> {
        self.0.token()
    }

    fn force_token_refresh(
        &mut self,
        #[zbus(signal_context)] ctxt: zbus::SignalContext<'_>,
    ) {
        self.0.force_token_refresh(ctxt)
    }
}
