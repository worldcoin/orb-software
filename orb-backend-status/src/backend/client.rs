use crate::{
    backend::types::{OrbStatusApiV2, VersionApiV2},
    collectors::connectivity::GlobalConnectivity,
};
use chrono::Utc;
use color_eyre::Result;
use derive_more::From;
use eyre::Context;
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::{Response, Url};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Extension};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use reqwest_tracing::{OtelName, TracingMiddleware};
use std::time::Duration;
use tokio::{
    sync::{oneshot, watch},
    task::{AbortHandle, JoinHandle},
};
use tracing::{error, info};

type ReqTx = (OrbStatusApiV2, oneshot::Sender<Result<Response, Err>>);

#[derive(From)]
pub enum Err {
    MissingAttestToken,
    NoConnectivity,
    #[from]
    Other(color_eyre::Report),
}

#[derive(Clone)]
pub struct StatusClient {
    handle: AbortHandle,
    req_tx: flume::Sender<ReqTx>,
}

impl Drop for StatusClient {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[bon::bon]
impl StatusClient {
    #[builder]
    pub fn new(
        orb_id: OrbId,
        orb_name: OrbName,
        jabil_id: OrbJabilId,
        orb_os_version: String,
        endpoint: Url,
        req_timeout: Duration,
        min_req_retry_interval: Duration,
        max_req_retry_interval: Duration,
        mut attest_token_rx: watch::Receiver<String>,
        mut connectivity_rx: watch::Receiver<GlobalConnectivity>,
    ) -> Self {
        info!("spawning backend-status client, orb_os_version: {orb_os_version}");

        let (req_tx, req_rx) = flume::unbounded::<ReqTx>();

        let handle: JoinHandle<Result<()>> = tokio::spawn(async move {
            let orb_id = orb_id.as_str().to_string();
            let orb_name = orb_name.to_string();
            let jabil_id = jabil_id.to_string();

            let make_client = || -> Result<ClientWithMiddleware> {
                let retry_policy = ExponentialBackoff::builder()
                    .retry_bounds(min_req_retry_interval, max_req_retry_interval)
                    .build_with_max_retries(3);

                let reqwest_client = reqwest::Client::builder()
                    .timeout(req_timeout)
                    .user_agent("orb-backend-status")
                    .build()
                    .wrap_err("failed to build reqwest client")?;

                let client = ClientBuilder::new(reqwest_client)
                    .with_init(Extension(OtelName(orb_id.clone().into())))
                    .with(TracingMiddleware::default())
                    .with(RetryTransientMiddleware::new_with_policy(retry_policy))
                    .build();

                Ok(client)
            };

            let mut client = make_client()
                .inspect_err(|e| error!("failed to create http client: {e:?}"))?;

            let mut attest_token = String::new();
            let mut connectivity = connectivity_rx.borrow_and_update().clone();

            info!("client with connectivity: {connectivity:?}");

            loop {
                tokio::select! {
                    biased;

                    Ok(_) = attest_token_rx.changed() => {
                        info!("new attest token received!");
                        let t = &attest_token_rx.borrow_and_update();
                        attest_token.clear();
                        attest_token.push_str(t);
                    }

                    Ok(_) = connectivity_rx.changed() => {
                        connectivity = connectivity_rx.borrow_and_update().clone();
                        if connectivity.is_connected() {
                            info!("connectivity status changed, rebuilding client");

                            client = make_client()
                                .inspect_err(|e| error!("failed to create http client: {e:?}"))?;
                        }
                    }

                    Ok((req, res_tx)) = req_rx.recv_async() => {
                        let res = if attest_token.is_empty() {
                            Err(Err::MissingAttestToken)
                        } else if !connectivity.is_connected() {
                            Err(Err::NoConnectivity)
                        } else {
                            let req = OrbStatusApiV2 {
                                orb_id: Some(orb_id.to_string()),
                                orb_name: Some(orb_name.to_string()),
                                jabil_id: Some(jabil_id.to_string()),
                                version: Some(VersionApiV2 {
                                    current_release: Some(orb_os_version.to_string()),
                                }),
                                timestamp: Utc::now(),
                                ..req
                            };

                            client
                                .post(endpoint.clone())
                                .json(&req)
                                .basic_auth(&orb_id, Some(attest_token.clone()))
                                .send()
                                .await
                                .wrap_err("failed to send request")
                                .map_err(Err::Other)
                        };

                        let _ = res_tx.send(res);
                    }
                };
            }
        });

        Self {
            handle: handle.abort_handle(),
            req_tx,
        }
    }

    pub async fn req(&self, payload: OrbStatusApiV2) -> Result<Response, Err> {
        let (tx, rx) = oneshot::channel();
        self.req_tx
            .send((payload, tx))
            .wrap_err("req_tx send failed")?;

        let res = rx.await.wrap_err("request oneshot failed")??;

        Ok(res)
    }
}
