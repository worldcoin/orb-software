use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agentwire::port;
use bon::builder;
use eyre::{Result, WrapErr as _};
use iroh::{endpoint::Connection, Endpoint};
use n0_future::FutureExt as _;
use n0_future::StreamExt as _;
use tokio::sync::oneshot;
use tracing::trace;

use crate::{
    handler::{BoxedHandler, ConnTx, Forwarder},
    Alpn, ConnectionTypeWatcher, EndpointConfig, FromAnyhow, RouterConfig,
};

#[derive(Debug, bon::Builder)]
pub struct Agent {
    #[builder(skip)]
    cancel_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    endpoint_cfg: EndpointConfig,
    router_cfg: RouterConfig,
}

impl agentwire::Agent for Agent {
    const NAME: &'static str = "iroh";
}

#[derive(Debug, thiserror::Error)]
#[error("agent failed to initialize")]
pub struct InitError(#[from] eyre::Report);

impl Agent {
    pub fn alpns(&self) -> impl Iterator<Item = Alpn> {
        self.router_cfg.handlers.keys().copied()
    }
}

impl agentwire::agent::Task for Agent {
    type Error = InitError;

    // #[tracing::instrument(name = "agent-iroh", skip_all)]
    async fn run(self, port: port::Inner<Self>) -> Result<(), Self::Error> {
        let endpoint = self.endpoint_cfg.bind().await?;

        let mut router = iroh::protocol::Router::builder(endpoint.clone());
        // Store these so we can clear the conn_tx later in response to [`Input`]s.
        let mut conn_tx_map = HashMap::with_capacity(self.router_cfg.handlers.len());
        for (alpn, handler) in self.router_cfg.handlers {
            let handler = BoxedHandler::from(handler);
            let conn_tx = Arc::new(Mutex::new(None));
            let forwarder = Forwarder::new(&endpoint, alpn, handler, &conn_tx);
            conn_tx_map.insert(alpn, conn_tx);
            router = router.accept(alpn.0, forwarder);
        }
        let router = router.spawn();

        let event_fut = handle_inputs()
            .port(port)
            .conn_tx_map(conn_tx_map)
            .endpoint(endpoint)
            .call();

        let cancel_rx = self.cancel_rx.unwrap();
        tokio::select! {
            result = event_fut => result?,
            _ = cancel_rx => {},
        }
        router
            .shutdown()
            .await
            .map_err(FromAnyhow)
            .wrap_err("failed to shutdown router")?;

        Ok(())
    }

    fn spawn_task(mut self) -> (port::Outer<Self>, agentwire::agent::Kill) {
        let name = <Self as agentwire::Agent>::NAME;
        let (inner, outer) = port::new();
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        self.cancel_rx = Some(cancel_rx);
        tokio::task::spawn(async move {
            tracing::info!("Agent {name} spawned");
            match self.run(inner).await {
                Ok(()) => {
                    tracing::warn!("Task agent {name} exited");
                }
                Err(err) => {
                    tracing::error!("Task agent {name} exited with error: {err:#?}",);
                }
            }
        });
        let kill_fut = async {
            let _ = cancel_tx.send(());
        };
        (outer, kill_fut.boxed())
    }
}

#[bon::builder]
async fn handle_inputs(
    mut port: port::Inner<Agent>,
    conn_tx_map: HashMap<Alpn, Arc<ConnTx>>,
    endpoint: Endpoint,
) -> Result<()> {
    while let Some(port::Input { value: input, .. }) = port.rx.next().await {
        trace!("got input {input:?}");
        match input {
            Input::Listen { alpn, conn_tx } => {
                let Some(arc_conn_tx) = conn_tx_map.get(&alpn) else {
                    panic!(
                        "only ALPNs registered when the agent was created can be used"
                    );
                };
                arc_conn_tx.lock().expect("poisoned").replace(conn_tx);
            }
            Input::Disable { alpn } => {
                let Some(arc_conn_tx) = conn_tx_map.get(&alpn) else {
                    panic!(
                        "only ALPNs registered when the agent was created can be used"
                    );
                };
                arc_conn_tx.lock().expect("poisoned").take();
            }
            Input::Connect {
                alpn,
                addr,
                conn_tx,
            } => {
                let endpoint = endpoint.clone();
                // Task because we don't want to block the event loop
                tokio::task::spawn(async move {
                    let conn_result = endpoint
                        .connect(addr.clone(), alpn.as_ref())
                        .await
                        .map_err(FromAnyhow)
                        .wrap_err("failed to connect")
                        .and_then(|conn| {
                            let conn_type = endpoint
                                .conn_type(addr.node_id)
                                .map_err(FromAnyhow)
                                .wrap_err("failed to get connection type")?;

                            Ok(ConnectionInfo { conn, conn_type })
                        });
                    let _ = conn_tx.send(conn_result);
                });
            }
        }
    }

    Ok(())
}

impl port::Port for Agent {
    type Input = Input;
    type Output = Output;

    const INPUT_CAPACITY: usize = 0;
    const OUTPUT_CAPACITY: usize = 0;
}

#[derive(Debug)]
pub enum Input {
    /// Listens to protocol `alpn` and drops any previously registered `conn_tx`
    /// If `alpn` is not one of the protocols registered when the agent was constructed,
    /// The code will panic.
    Listen {
        alpn: Alpn,
        conn_tx: flume::Sender<ConnectionInfo>,
    },
    /// Disables the listeners for `alpn` and drops any previously registered `conn_tx`.
    Disable { alpn: Alpn },
    /// Connect to a peer
    Connect {
        alpn: Alpn,
        addr: iroh::NodeAddr,
        conn_tx: oneshot::Sender<Result<ConnectionInfo>>,
    },
}

#[derive(Debug)]
pub struct ConnectionInfo {
    pub conn: Connection,
    pub conn_type: ConnectionTypeWatcher,
}

#[derive(Debug)]
pub enum Output {}
