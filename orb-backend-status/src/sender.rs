use crate::backend::client::{self, StatusClient};
use crate::backend::types::OrbStatusApiV2;
use crate::collectors::Collectors;
use crate::orb_event_stream::OrbEventStream;
use color_eyre::eyre::Result;
use std::time::Duration;
use tokio::time::{self};
use tokio_util::sync::CancellationToken;
use tracing::error;

#[derive(Clone)]
pub struct BackendSender {
    client: StatusClient,
    interval: Duration,
    oes: OrbEventStream,
}

impl BackendSender {
    pub fn new(client: StatusClient, oes: OrbEventStream, interval: Duration) -> Self {
        Self {
            client,
            oes,
            interval,
        }
    }

    pub async fn send_snapshot(&self, mut req: OrbStatusApiV2) -> Result<bool> {
        req.oes_cached = true;
        req.oes = Some(self.oes.cached()?);

        let res = match self.client.req(req).await {
            Err(client::Err::MissingAttestToken | client::Err::NoConnectivity) => {
                return Ok(false);
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

        Ok(true)
    }

    pub async fn run_loop(
        self,
        collectors: Collectors,
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
                _ = collectors.wait_for_urgent_send() => (),
            };

            let req = collectors.snapshot().await;

            match self.send_snapshot(req).await {
                Ok(sent) => {
                    if sent {
                        collectors.clear_send_immediately();
                        interval.reset();
                    }
                }

                Err(e) => {
                    error!("failed to send status : {e:?}");
                }
            };
        }
    }
}
