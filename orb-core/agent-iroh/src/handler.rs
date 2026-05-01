use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use iroh::{
    protocol::{AcceptError, DynProtocolHandler, ProtocolHandler},
    Endpoint,
};

use crate::{agent::ConnectionInfo, Alpn};

const ACCEPT_TIMEOUT: Duration = Duration::from_millis(5000);

#[derive(Debug, derive_more::From)]
pub struct BoxedHandler(Box<dyn DynProtocolHandler>);

impl ProtocolHandler for BoxedHandler {
    fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> impl std::future::Future<Output = Result<(), AcceptError>> + Send {
        self.0.accept(connection)
    }

    fn shutdown(
        &self,
    ) -> impl std::future::Future<Output = ()> + Send {
        self.0.shutdown()
    }
}

#[derive(Debug)]
pub(crate) struct Forwarder<T: ProtocolHandler> {
    _endpoint: Endpoint,
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
            _endpoint: endpoint.clone(),
            alpn,
            handler: Arc::new(handler),
            conn_tx: conn_tx.clone(),
        }
    }
}

fn io_err(msg: impl std::fmt::Display) -> AcceptError {
    AcceptError::from_err(std::io::Error::other(msg.to_string()))
}

impl<T: ProtocolHandler> ProtocolHandler for Forwarder<T> {
    fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> impl std::future::Future<Output = Result<(), AcceptError>> + Send {
        let paths = connection.paths();
        let handler = self.handler.clone();
        let arc_conn_tx = self.conn_tx.clone();
        let alpn = self.alpn;
        async move {
            if arc_conn_tx.lock().expect("poisoned").is_none() {
                return Err(io_err(format_args!(
                    "not accepting connections on alpn {alpn}"
                )));
            };
            let accept_result = tokio::time::timeout(
                ACCEPT_TIMEOUT,
                ProtocolHandler::accept(&*handler, connection.clone()),
            )
            .await;
            match accept_result {
                Err(_elapsed) => {
                    return Err(io_err(format_args!(
                        "timeout in accept for alpn {alpn}"
                    )));
                }
                Ok(Err(e)) => {
                    return Err(io_err(format_args!(
                        "error in accept for alpn {alpn}: {e}"
                    )));
                }
                Ok(Ok(())) => {}
            }

            let Some(ref mut conn_tx) = *arc_conn_tx.lock().expect("poisoned") else {
                return Err(io_err(format_args!(
                    "not accepting connections on alpn {alpn}"
                )));
            };
            conn_tx.try_send(ConnectionInfo {
                conn: connection,
                paths,
            }).map_err(|_| {
                io_err(format_args!("too many concurrent connections"))
            })?;

            Ok(())
        }
    }
}
