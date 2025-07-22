use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use eyre::{eyre, WrapErr as _};
use iroh::{protocol::ProtocolHandler, Endpoint};
use n0_future::{boxed::BoxFuture, FutureExt, TryFutureExt as _};

use crate::{agent::ConnectionInfo, Alpn, FromAnyhow, FromEyre};

const ACCEPT_TIMEOUT: Duration = Duration::from_millis(5000);

#[derive(Debug, derive_more::From, derive_more::Into)]
pub struct BoxedHandler(Box<dyn ProtocolHandler>);

impl ProtocolHandler for BoxedHandler {
    fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> BoxFuture<anyhow::Result<()>> {
        self.0.accept(connection)
    }

    fn on_connecting(
        &self,
        connecting: iroh::endpoint::Connecting,
    ) -> BoxFuture<anyhow::Result<iroh::endpoint::Connection>> {
        self.0.on_connecting(connecting)
    }

    fn shutdown(&self) -> BoxFuture<()> {
        self.0.shutdown()
    }
}

#[derive(Debug)]
pub(crate) struct Forwarder<T: ProtocolHandler> {
    endpoint: Endpoint,
    alpn: Alpn,
    handler: Arc<T>, // arc so that the future doesn't borrow from self
    conn_tx: Arc<ConnTx>,
}

pub(crate) type ConnTx = Mutex<Option<flume::Sender<ConnectionInfo>>>;

impl<T: ProtocolHandler> Forwarder<T> {
    pub fn new(
        endpoint: &Endpoint,
        alpn: Alpn,
        handler: T,
        conn_tx: &Arc<ConnTx>,
    ) -> Self {
        Self {
            endpoint: endpoint.clone(),
            alpn,
            handler: Arc::new(handler),
            conn_tx: conn_tx.clone(),
        }
    }
}

impl<T: ProtocolHandler> ProtocolHandler for Forwarder<T> {
    fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> BoxFuture<anyhow::Result<()>> {
        let conn_type = match connection
            .remote_node_id()
            .and_then(|node_id| self.endpoint.conn_type(node_id))
        {
            Ok(watcher) => watcher,
            Err(err) => return std::future::ready(Err(err)).boxed(),
        };
        let handler = self.handler.clone();
        let arc_conn_tx = self.conn_tx.clone();
        let alpn = self.alpn;
        let fut = async move {
            if arc_conn_tx.lock().expect("poisoned").is_none() {
                return Err(eyre!("not accepting connections on alpn {alpn}"));
            };
            tokio::time::timeout(
                ACCEPT_TIMEOUT,
                handler.accept(connection.clone()).map_err(FromAnyhow),
            )
            .await
            .wrap_err_with(|| format!("timeout in accept for alpn {alpn}"))?
            .wrap_err_with(|| format!("error in accept for alpn {alpn}"))?;

            let Some(ref mut conn_tx) = *arc_conn_tx.lock().expect("poisoned") else {
                return Err(eyre!("not accepting connections on alpn {alpn}"));
            };
            conn_tx
                .try_send(ConnectionInfo {
                    conn: connection,
                    conn_type,
                })
                .wrap_err("too many concurrent connections")?;

            Ok(())
        };

        fut.map_err(FromEyre).map_err(anyhow::Error::new).boxed()
    }
}
