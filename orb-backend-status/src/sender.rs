use crate::backend::client::{self, StatusClient};
use crate::dbus::intf_impl::CurrentStatus;
use crate::oes_cache::OesEventCache;
use color_eyre::eyre::Result;
use std::time::Duration;
use tokio::time::{self};
use tokio_util::sync::CancellationToken;
use tracing::error;

#[derive(Clone)]
pub struct BackendSender {
    client: StatusClient,
    interval: Duration,
    oes_cache: OesEventCache,
}

impl BackendSender {
    pub fn new(
        client: StatusClient,
        oes_cache: OesEventCache,
        interval: Duration,
    ) -> Self {
        Self {
            client,
            oes_cache,
            interval,
        }
    }

    pub async fn send_snapshot(&self, snapshot: &CurrentStatus) -> Result<()> {
        let mut req = snapshot.to_orb_status_api_v2_req().await;
        req.oes_cached = true;
        req.oes = Some(self.oes_cache.values()?);

        let res = match self.client.req(req).await {
            Err(client::Err::MissingAttestToken | client::Err::NoConnectivity) => {
                return Ok(());
            }

            Err(client::Err::Other(e)) => return Err(e),

            Ok(res) => res,
        };

        let status = res.status();
        if !status.is_success() {
            let response_body = res.text().await.unwrap_or_default();
            return Err(eyre::eyre!(
                "Backend status error: {} - {}",
                status,
                response_body
            ));
        }

        Ok(())
    }

    pub async fn run_loop(
        self,
        backend_status: crate::dbus::intf_impl::BackendStatusImpl,
        shutdown_token: CancellationToken,
    ) {
        let mut interval = time::interval(self.interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => break,

                // Periodic interval (30 seconds)
                _ = interval.tick() => (),

                // Something urgent happened (reboot or SSID change)
                _ = backend_status.wait_for_urgent_send() => (),
            };

            let snapshot = backend_status.snapshot();

            match self.send_snapshot(&snapshot).await {
                Ok(_) => {
                    backend_status.clear_send_immediately();
                    interval.reset();
                }

                Err(e) => {
                    error!("failed to send status : {e:?}");
                }
            };
        }
    }
}
