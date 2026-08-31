use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use iroh::{
    endpoint::{Connecting, Connection},
    protocol::{AcceptError, DynProtocolHandler, ProtocolHandler},
    Endpoint,
};

use crate::{agent::ConnectionInfo, Alpn};

const ACCEPT_TIMEOUT: Duration = Duration::from_millis(5000);

#[derive(Debug, derive_more::From)]
pub struct BoxedHandler(Box<dyn DynProtocolHandler>);

impl ProtocolHandler for BoxedHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        self.0.accept(connection).await
    }

    async fn on_connecting(
        &self,
        connecting: Connecting,
    ) -> Result<Connection, AcceptError> {
        self.0.on_connecting(connecting).await
    }

    async fn shutdown(&self) {
        self.0.shutdown().await
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
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let conn_type = connection
            .remote_node_id()
            .map_err(|source| AcceptError::MissingRemoteNodeId { source })
            .and_then(|node_id| {
                self.endpoint.conn_type(node_id).ok_or_else(|| {
                    AcceptError::from(std::io::Error::other("no connection type available"))
                })
            })?;

        let handler = self.handler.clone();
        let arc_conn_tx = self.conn_tx.clone();
        let alpn = self.alpn;

        if arc_conn_tx.lock().expect("poisoned").is_none() {
            return Err(AcceptError::from(std::io::Error::other(format!(
                "not accepting connections on alpn {alpn}"
            ))));
        };

        tokio::time::timeout(ACCEPT_TIMEOUT, ProtocolHandler::accept(&*handler, connection.clone()))
            .await
            .map_err(|_| {
                AcceptError::from(std::io::Error::other(format!(
                    "timeout in accept for alpn {alpn}"
                )))
            })??;

        let Some(ref mut conn_tx) = *arc_conn_tx.lock().expect("poisoned") else {
            return Err(AcceptError::from(std::io::Error::other(format!(
                "not accepting connections on alpn {alpn}"
            ))));
        };
        conn_tx.try_send(ConnectionInfo { conn: connection, conn_type }).map_err(|e| {
            AcceptError::from(std::io::Error::other(format!(
                "too many concurrent connections: {e}"
            )))
        })?;

        Ok(())
    }
}
