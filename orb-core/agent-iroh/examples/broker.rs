mod common;

use std::task::Context;
use std::task::Poll;
use std::time::Instant;

use agentwire::{port, Broker, BrokerFlow};
use color_eyre::Result;
use eyre::WrapErr as _;
use futures::SinkExt;
use iroh::NodeAddr;
use n0_future::StreamExt;
use orb_agent_iroh::agent::ConnectionInfo;
use orb_agent_iroh::EndpointConfig;
use orb_agent_iroh::RouterConfig;
use orb_agent_iroh::{agent::Input, Agent as IrohAgent, Alpn};
use tokio::sync::oneshot;
use tracing::{debug, info, level_filters::LevelFilter, trace};
use tracing_subscriber::EnvFilter;

use crate::common::{phone_pubkey, AppProtocol};

#[allow(missing_docs, clippy::struct_excessive_bools)]
#[derive(Broker)]
#[broker(plan = PlanT, error = eyre::Report, poll_extra)]
/// The main broker for the app.
struct Broker {
    #[agent(task, init)]
    pub iroh: agentwire::agent::Cell<IrohAgent>,
}

impl Broker {
    fn new() -> Self {
        debug!("Broker::new called");
        new_broker!()
    }

    fn init_iroh(&mut self) -> IrohAgent {
        debug!("Broker::init_iroh called");
        orb_agent_iroh::Agent::builder()
            .endpoint_cfg(EndpointConfig::builder().build())
            .router_cfg(
                RouterConfig::builder()
                    .handler(AppProtocol::ALPN, AppProtocol)
                    .build(),
            )
            .build()
    }

    fn handle_iroh(
        &mut self,
        _plan: &mut dyn PlanT,
        _output: port::Output<IrohAgent>,
    ) -> Result<BrokerFlow> {
        trace!("Broker::handle_iroh called");
        Ok(BrokerFlow::Continue)
    }

    // Exists merely to work around a bug:
    // https://github.com/worldcoin/orb-software/pull/554
    fn poll_extra(
        &mut self,
        _plan: &mut dyn PlanT,
        _cx: &mut Context,
        _fence: Instant,
    ) -> Result<Option<Poll<()>>> {
        Ok(Some(Poll::Pending))
    }
}

trait PlanT {}

struct Plan {
    fence: Instant,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            fence: Instant::now(),
        }
    }
}

impl PlanT for Plan {}

impl Plan {
    async fn run_pre(&mut self, broker: &mut Broker) -> Result<()> {
        self.fence = Instant::now();

        broker.enable_iroh()?;

        Ok(())
    }

    async fn run(&mut self, broker: &mut Broker) -> Result<()> {
        self.run_pre(broker).await?;

        // This is as if the pubkey was in the QR code UserData
        let phone_addr = NodeAddr::new(phone_pubkey());
        let start = Instant::now();
        let ConnectionInfo {
            conn: conn_to_phone,
            conn_type,
        } = self.connect(broker, AppProtocol::ALPN, phone_addr).await?;
        tokio::task::spawn(async move {
            let mut stream = conn_type.stream();
            while let Some(event) = stream.next().await {
                info!("connection type changed: {event:?}");
            }
        });

        let elapsed = start.elapsed();
        tracing::info!("connect latency: {elapsed:?}");

        // send 4KiB blob
        let payload: Vec<u8> = (0..u8::MAX).cycle().take(1024 * 4).collect();
        assert_eq!(payload.len(), 1024 * 4);
        info!("sending 4KiB blob to phone...");
        let start = Instant::now();
        let mut send_stream = conn_to_phone.open_uni().await?;
        let after_stream_open = start.elapsed();
        send_stream.write_all(payload.as_slice()).await?;
        let after_write = start.elapsed();
        send_stream.finish()?;
        let after_flush = start.elapsed();
        assert_eq!(send_stream.stopped().await?, None);
        let after_ack = start.elapsed();

        tracing::info!(
            ?after_stream_open,
            ?after_write,
            ?after_flush,
            ?after_ack,
            "latency of 4KiB"
        );

        self.run_post(broker).await;
        Ok(())
    }

    async fn run_post(&mut self, broker: &mut Broker) {
        broker.disable_iroh();
    }

    async fn connect(
        &mut self,
        broker: &mut Broker,
        alpn: Alpn,
        addr: impl Into<NodeAddr>,
    ) -> Result<ConnectionInfo> {
        let addr = addr.into();
        let outer_port = broker.iroh.enabled().unwrap();
        let (tx, rx) = oneshot::channel();
        outer_port
            .tx
            .send(port::Input::new(Input::Connect {
                alpn,
                addr: addr.clone(),
                conn_tx: tx,
            }))
            .await
            .wrap_err("agent dead")?;
        let fence = self.fence;
        let run_fut = broker.run_with_fence(self, fence);
        let conn_info = tokio::select! {
            result = run_fut => {
                result.wrap_err("broker errored")?;
                unreachable!("broker never terminates")
            },
            result = rx => match result {
                Err(_) => unreachable!("sending channel never is dropped"),
                Ok(conn) => conn,
            }
        };

        conn_info.wrap_err_with(|| {
            format!("error while connecting to {addr:?} on alpn {alpn}")
        })
    }

    /// Currently unused, but demonstrates how to listen to connecting peers (as if
    /// we were a server).
    #[expect(dead_code)]
    async fn wait_for_connection(
        &mut self,
        broker: &mut Broker,
        alpn: Alpn,
    ) -> Result<ConnectionInfo> {
        let (tx, rx) = flume::bounded(1);
        let outer_port = broker.iroh.enabled().unwrap();
        outer_port
            .tx
            .send(port::Input::new(Input::Listen { alpn, conn_tx: tx }))
            .await
            .wrap_err("agent dead")?;
        let fence = self.fence;
        let run_fut = broker.run_with_fence(self, fence);
        let conn_info = tokio::select! {
            result = run_fut => {
                result.wrap_err("broker errored")?;
                unreachable!("broker never terminates")
            },
            result = rx.recv_async() => match result {
                Err(flume::RecvError::Disconnected) => unreachable!("sending channel never is dropped"),
                Ok(conn) => conn,
            }
        };

        Ok(conn_info)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    info!("starting");

    let mut broker = Broker::new();
    let mut plan = Plan::default();
    plan.run(&mut broker)
        .await
        .wrap_err("error while running plan")?;

    info!("exiting");

    Ok(())
}
