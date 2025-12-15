use crate::backend::status::StatusClient;
use crate::dbus::intf_impl::CurrentStatus;
use color_eyre::eyre::Result;
use tracing::info;

#[derive(Clone)]
pub struct BackendSender {
    client: StatusClient,
}

impl BackendSender {
    pub fn new(client: StatusClient) -> Self {
        Self { client }
    }

    pub async fn send_snapshot(
        &self,
        snapshot: &CurrentStatus,
        token: &str,
    ) -> Result<()> {
        if token.is_empty() {
            info!("auth token not available yet - skipping send");
            return Ok(());
        }

        self.client.send_status(snapshot, token).await
    }
}


