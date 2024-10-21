//! Dbus interface definitions.

use zbus::interface;

use crate::context::Context;

pub struct Interface {
    ctx: Context,
}

impl Interface {
    pub(crate) fn new(ctx: Context) -> Self {
        Self { ctx }
    }
}

#[interface(name = "org.worldcoin.BackendState1")]
impl Interface {
    /// Retrieves the cached state of the orb
    #[zbus(property)]
    fn state(&self) -> zbus::fdo::Result<String> {
        match self.ctx.state.get_cloned().as_deref() {
            Some("") => Err(zbus::fdo::Error::Failed(
                "state was set, but is an empty string".into(),
            )),
            Some(state) => Ok(state.to_string()),
            None => Err(zbus::fdo::Error::Failed(
                "state was not yet or could not be retrieved from backend".into(),
            )),
        }
    }

    /// Forces a request to the backend for the latest state.
    async fn refresh_state(&self) -> zbus::fdo::Result<String> {
        match crate::update_state(&self.ctx).await {
            Ok(s) => Ok(s.into()),
            Err(e) => {
                tracing::error!(err=?e, "failed to refresh state");
                Err(zbus::fdo::Error::Failed(format!("{e:?}")))
            }
        }
    }
}
