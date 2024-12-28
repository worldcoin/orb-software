//! Orb-Relay client
use crate::{debug_any, IntoPayload, PayloadMatcher};
use eyre::{Context, OptionExt, Result};
use orb_relay_messages::{
    common,
    prost_types::Any,
    relay::{
        connect_request::AuthMethod, entity::EntityType, relay_connect_request,
        relay_connect_response, relay_service_client::RelayServiceClient,
        ConnectRequest, ConnectResponse, Entity, Heartbeat, RelayConnectRequest,
        RelayConnectResponse, RelayPayload, ZkpAuthRequest,
    },
    self_serve,
    tonic::{
        transport::{Certificate, Channel, ClientTlsConfig},
        Streaming,
    },
};
use orb_security_utils::reqwest::{
    AWS_ROOT_CA1_CERT, AWS_ROOT_CA2_CERT, AWS_ROOT_CA3_CERT, AWS_ROOT_CA4_CERT,
    GTS_ROOT_R1_CERT, GTS_ROOT_R2_CERT, GTS_ROOT_R3_CERT, GTS_ROOT_R4_CERT,
    SFS_ROOT_G2_CERT,
};
use std::{
    any::type_name,
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use tokio::{
    sync::{
        mpsc::{self, Sender},
        oneshot, Mutex,
    },
    time::{self, Duration},
};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct TokenAuth {
    token: String,
}

#[derive(Debug, Clone)]
pub struct ZkpAuth {
    root: String,
    signal: String,
    nullifier_hash: String,
    proof: String,
}

#[derive(Debug, Clone)]
pub enum Auth {
    Token(TokenAuth),
    ZKP(ZkpAuth),
}

#[derive(Debug, Clone)]
enum Mode {
    Orb,
    App,
}

#[derive(Debug, Clone)]
struct Config {
    src_id: String,
    dst_id: String,
    url: String,

    auth: Auth,

    // TODO: Maybe split this into a separate struct and a trait?
    mode: Mode,

    max_buffer_size: usize,
    reconnect_delay: Duration,
    keep_alive_interval: Duration,
    keep_alive_timeout: Duration,
    connect_timeout: Duration,
    request_timeout: Duration,
    heartbeat_interval: Duration,
}

enum Command {
    ReplayPendingMessages,
    GetPendingMessages(oneshot::Sender<usize>),
    Reconnect,
}

enum OutgoingMessage {
    Normal(Any),
    Blocking(Any, oneshot::Sender<()>),
}

/// Client state
pub struct Client {
    message_buffer: Arc<Mutex<VecDeque<RelayPayload>>>,
    outgoing_tx: Option<mpsc::Sender<OutgoingMessage>>,
    command_tx: Option<mpsc::Sender<Command>>,
    shutdown_token: Option<CancellationToken>,
    shutdown_completed: Option<oneshot::Receiver<()>>,
    config: Config,
}

impl Client {
    fn no_state(&self) -> RelayConnectRequest {
        let (src_t, dst_t) = match self.config.mode {
            Mode::Orb => (EntityType::Orb as i32, EntityType::App as i32),
            Mode::App => (EntityType::App as i32, EntityType::Orb as i32),
        };
        RelayPayload {
            src: Some(Entity {
                id: self.config.src_id.clone(),
                entity_type: src_t,
            }),
            dst: Some(Entity {
                id: self.config.dst_id.clone(),
                entity_type: dst_t,
            }),
            payload: Some(common::v1::NoState::default().into_payload()),
            seq: 0,
        }
        .into()
    }

    #[must_use]
    fn new(
        url: String,
        auth: Auth,
        src_id: String,
        dst_id: String,
        mode: Mode,
    ) -> Self {
        Self {
            message_buffer: Arc::new(Mutex::new(VecDeque::new())),
            outgoing_tx: None,
            command_tx: None,
            shutdown_token: None,
            shutdown_completed: None,
            config: Config {
                src_id,
                dst_id,
                url,
                auth,
                mode,
                max_buffer_size: 100,
                reconnect_delay: Duration::from_secs(1),
                keep_alive_interval: Duration::from_secs(5),
                keep_alive_timeout: Duration::from_secs(10),
                connect_timeout: Duration::from_secs(20),
                request_timeout: Duration::from_secs(20),
                heartbeat_interval: Duration::from_secs(15),
            },
        }
    }

    /// Create a new client that sends messages from an Orb to an App
    #[must_use]
    pub fn new_as_orb(
        url: String,
        token: String,
        orb_id: String,
        session_id: String,
    ) -> Self {
        Self::new(
            url,
            Auth::Token(TokenAuth { token }),
            orb_id,
            session_id,
            Mode::Orb,
        )
    }

    /// Create a new client that sends messages from an App to an Orb
    #[must_use]
    pub fn new_as_app(
        url: String,
        token: String,
        session_id: String,
        orb_id: String,
    ) -> Self {
        Self::new(
            url,
            Auth::Token(TokenAuth { token }),
            session_id,
            orb_id,
            Mode::App,
        )
    }

    /// Create a new client that sends messages from an App to an Orb (using ZKP as auth method)
    #[must_use]
    pub fn new_as_app_zkp(
        url: String,
        root: String,
        signal: String,
        nullifier_hash: String,
        proof: String,
        session_id: String,
        orb_id: String,
    ) -> Self {
        Self::new(
            url,
            Auth::ZKP(ZkpAuth {
                root,
                signal,
                nullifier_hash,
                proof,
            }),
            session_id,
            orb_id,
            Mode::App,
        )
    }

    async fn check_for_msg<T: PayloadMatcher>(&self) -> Option<T::Output> {
        for msg in self.get_buffered_messages().await {
            if let Some(payload) = &msg.payload {
                if let Some(specific_payload) = T::matches(payload) {
                    return Some(specific_payload);
                }
                tracing::warn!(
                    "While waiting for payload of type {:?}, we got: {:?}",
                    type_name::<T>(),
                    debug_any(&msg.payload)
                );
            }
        }
        None
    }

    /// Get buffered messages
    pub async fn get_buffered_messages(&self) -> VecDeque<RelayPayload> {
        let mut buffer = self.message_buffer.lock().await;
        std::mem::take(&mut *buffer)
    }

    /// Connect to the Orb-Relay server
    pub async fn connect(&mut self) -> Result<()> {
        let shutdown_token = CancellationToken::new();
        self.shutdown_token = Some(shutdown_token.clone());

        let (connection_established_tx, connection_established_rx) = oneshot::channel();

        let message_buffer = Arc::clone(&self.message_buffer);
        // TODO: Make the buffer size configurable
        let (outgoing_tx, mut outgoing_rx) = mpsc::channel(32);
        self.outgoing_tx = Some(outgoing_tx);
        let (command_tx, mut command_rx) = mpsc::channel(32);
        self.command_tx = Some(command_tx);
        let (shutdown_completed_tx, shutdown_completed_rx) = oneshot::channel();
        self.shutdown_completed = Some(shutdown_completed_rx);

        let config = self.config.clone();
        let no_state = self.no_state();

        tracing::info!(
            "Connecting with: src_id: {}, dst_id: {}",
            config.src_id,
            config.dst_id
        );
        tokio::spawn(async move {
            let mut agent = PollerAgent {
                config: &config,
                pending_messages: Default::default(),
                last_message: no_state,
                seq: 0,
            };
            let mut connection_established_tx = Some(connection_established_tx);

            loop {
                if let Err(e) = agent
                    .main_loop(
                        &message_buffer,
                        shutdown_token.clone(),
                        &mut outgoing_rx,
                        &mut command_rx,
                        connection_established_tx.take(),
                    )
                    .await
                {
                    tracing::error!("Connection error: {e}");
                }

                if shutdown_token.is_cancelled() {
                    tracing::info!("Connection shutdown");
                    break;
                }

                tracing::info!(
                    "Reconnecting in {}s ...",
                    config.reconnect_delay.as_secs()
                );
                tokio::time::sleep(config.reconnect_delay).await;
            }
            shutdown_completed_tx.send(()).ok();
        });

        // Wait for the connection to be established. Notice that if the first connection attempt, this will pop an
        // error as expected behavior.
        connection_established_rx
            .await
            .wrap_err("Failed to establish connection")?;

        Ok(())
    }

    /// Wait for a specific message type
    pub async fn wait_for_msg<T: PayloadMatcher>(
        &self,
        wait: Duration,
    ) -> Result<T::Output> {
        let start_time = tokio::time::Instant::now();
        loop {
            if let Some(payload) = self.check_for_msg::<T>().await {
                return Ok(payload);
            }
            if start_time.elapsed() >= wait {
                return Err(eyre::eyre!(
                    "Timeout waiting for payload of type {:?}",
                    std::any::type_name::<T>()
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Send a message to current session
    pub async fn send<T: IntoPayload>(&mut self, msg: T) -> Result<()> {
        self.send_internal(msg, None).await
    }

    /// Send a message and wait until the corresponding ack is received
    pub async fn send_blocking<T: IntoPayload>(
        &mut self,
        msg: T,
        timeout: Duration,
    ) -> Result<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.send_internal(msg, Some(ack_tx)).await?;
        match tokio::time::timeout(timeout, ack_rx).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(eyre::eyre!("Failed to receive ack: sender dropped")),
            Err(_) => Err(eyre::eyre!("Timeout waiting for ack")),
        }
    }

    async fn send_internal<T: IntoPayload>(
        &mut self,
        msg: T,
        ack_tx: Option<oneshot::Sender<()>>,
    ) -> Result<()> {
        let msg = match ack_tx {
            Some(ack_tx) => OutgoingMessage::Blocking(msg.into_payload(), ack_tx),
            None => OutgoingMessage::Normal(msg.into_payload()),
        };
        self.outgoing_tx
            .as_ref()
            .ok_or_eyre("client not connected")?
            .send(msg)
            .await
            .inspect_err(|e| tracing::error!("Failed to send payload: {e}"))
            .wrap_err("Failed to send payload")
    }

    /// Check if there are any pending messages
    pub async fn has_pending_messages(&self) -> Result<usize> {
        let command_tx = self
            .command_tx
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Client not connected"))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        command_tx
            .send(Command::GetPendingMessages(reply_tx))
            .await?;
        let pending_count = reply_rx.await?;
        Ok(pending_count)
    }

    /// Request to replay pending messages
    pub async fn replay_pending_messages(&self) -> Result<()> {
        let command_tx = self
            .command_tx
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Client not connected"))?;
        command_tx.send(Command::ReplayPendingMessages).await?;
        Ok(())
    }

    /// Reconnect the client. On restart, pending messages will be replayed.
    pub async fn reconnect(&self) -> Result<()> {
        let command_tx = self
            .command_tx
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Client not connected"))?;
        command_tx.send(Command::Reconnect).await?;
        Ok(())
    }

    pub async fn graceful_shutdown(
        &mut self,
        wait_for_pending_messages: Duration,
        wait_for_shutdown: Duration,
    ) {
        // Let's wait for all acks to be received
        if self.has_pending_messages().await.map_or(false, |n| n > 0) {
            tracing::info!(
                "Giving {}ms for pending messages to be acked",
                wait_for_pending_messages.as_millis()
            );
            tokio::time::sleep(wait_for_pending_messages).await;
        }
        // If there are still pending messages, we retry to send them
        if self.has_pending_messages().await.map_or(false, |n| n > 0) {
            tracing::info!("There are still pending messages, replaying...");
            if let Ok(()) = self.replay_pending_messages().await {
                tokio::time::sleep(wait_for_pending_messages).await;
            }
        }

        // Eventually, there not much more we can do, so we shutdown the client
        self.shutdown();

        if let Some(shutdown_completed) = self.shutdown_completed.take() {
            match tokio::time::timeout(wait_for_shutdown, shutdown_completed).await {
                Ok(_) => tracing::info!("Shutdown completed successfully."),
                Err(_) => tracing::warn!("Timed out waiting for shutdown to complete."),
            }
        }
    }

    /// Shutdown the client
    pub fn shutdown(&mut self) {
        tracing::info!("Shutting down requested");
        if let Some(token) = self.shutdown_token.take() {
            token.cancel();
        }
    }

    pub async fn wait_for_msg_while_spamming<
        T: PayloadMatcher,
        S: IntoPayload + std::clone::Clone,
    >(
        &mut self,
        wait: Duration,
        spam: S,
        spam_every: Duration,
    ) -> Result<T::Output> {
        let start_time = tokio::time::Instant::now();
        let mut spam_time = tokio::time::Instant::now();
        loop {
            if let Some(payload) = self.check_for_msg::<T>().await {
                return Ok(payload);
            }

            if spam_time.elapsed() >= spam_every {
                let _ = self.send(spam.clone()).await;
                spam_time = tokio::time::Instant::now();
            }

            if start_time.elapsed() >= wait {
                return Err(eyre::eyre!(
                    "Timeout waiting for payload of type {:?}",
                    std::any::type_name::<T>()
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct PollerAgent<'a> {
    config: &'a Config,
    pending_messages: BTreeMap<u64, (RelayConnectRequest, Option<oneshot::Sender<()>>)>,
    last_message: RelayConnectRequest,
    seq: u64,
}

impl<'a> PollerAgent<'a> {
    // TODO: We need to split auth and subscription. Maybe ideally we issue 1 connect and then a subscribe that will notify
    // the server that we care about messages from a certain queue only. That will avoid multiplexing messages from
    // different sources.
    async fn main_loop(
        &mut self,
        message_buffer: &Arc<Mutex<VecDeque<RelayPayload>>>,
        shutdown_token: CancellationToken,
        outgoing_rx: &mut mpsc::Receiver<OutgoingMessage>,
        command_rx: &mut mpsc::Receiver<Command>,
        connection_established_tx: Option<oneshot::Sender<()>>,
    ) -> Result<()> {
        let (mut response_stream, sender_tx) = match self.connect().await {
            Ok(ok) => ok,
            Err(e) => return Err(e),
        };

        if let Some(tx) = connection_established_tx {
            let _ = tx.send(());
        }

        self.replay_pending_messages(&sender_tx).await?;

        let mut interval = time::interval(self.config.heartbeat_interval);

        loop {
            tokio::select! {
                () = shutdown_token.cancelled() => {
                    tracing::info!("Shutting down connection");
                    if !self.pending_messages.is_empty() {
                        tracing::warn!("Pending messages {}: {:?}", self.pending_messages.len(), self.pending_messages);
                    }
                    return Ok(());
                }
                message = response_stream.next() => {
                    match message {
                        Some(Ok(RelayConnectResponse {
                            msg:
                                Some(relay_connect_response::Msg::Payload(RelayPayload {
                                    src: Some(src),
                                    dst,
                                    seq,
                                    payload: Some(payload),
                                })),
                        })) => {
                            if self_serve::app::v1::RequestState::matches(&payload).is_some() {
                                sender_tx
                                    .send(self.last_message.clone())
                                    .await
                                    .wrap_err("Failed to send outgoing message")?;
                            } else if src.id != self.config.dst_id {
                                tracing::error!(
                                    "Skipping received message from unexpected source: {:?}: {payload:?}",
                                    src.id
                                );
                            } else {
                                self.handle_message(
                                    RelayPayload { src: Some(src), dst, seq, payload: Some(payload) },
                                    message_buffer,
                                )
                                .await?;
                            }
                        }
                        Some(Ok(RelayConnectResponse { msg: Some(relay_connect_response::Msg::Ack(ack)) })) => {
                            if let Some((_, Some(ack_tx))) = self.pending_messages.remove(&ack.seq) {
                                if ack_tx.send(()).is_err() {
                                    // The receiver has been dropped, possibly due to a timeout. That means we
                                    // need to increase the timeout at send_blocking().
                                    tracing::warn!(
                                        "Failed to send ack back to send_blocking(): receiver dropped"
                                    );
                                }
                            }
                        }
                        Some(Err(e)) => {
                            tracing::error!("Error receiving message from tonic stream: {e:?}");
                            return Err(e.into());
                        }
                        None => {
                            tracing::info!("Stream ended");
                            return Ok(());
                        }
                        _ => {
                            tracing::error!("Received unexpected message: {message:?}");
                        }
                    }
                }
                Some(outgoing_message) = outgoing_rx.recv() => {
                    self.seq = self.seq.wrapping_add(1);
                    let (payload, maybe_ack_tx) = match outgoing_message {
                        OutgoingMessage::Normal(payload) => (payload, None),
                        OutgoingMessage::Blocking(payload, ack_tx) => (payload, Some(ack_tx)),
                    };
                    let (src_t, dst_t) = match self.config.mode {
                        Mode::Orb => (EntityType::Orb as i32, EntityType::App as i32),
                        Mode::App => (EntityType::App as i32, EntityType::Orb as i32),
                    };
                    let relay_message = RelayPayload {
                        src: Some(Entity { id: self.config.src_id.clone(), entity_type: src_t }),
                        dst: Some(Entity { id: self.config.dst_id.clone(), entity_type: dst_t }),
                        seq: self.seq,
                        payload:  Some(payload),
                    };

                    tracing::debug!("Sending message: from: {:?}, to: {:?}, seq: {:?}, payload: {:?}",
                        relay_message.src, relay_message.dst, relay_message.seq, debug_any(&relay_message.payload));

                    self.pending_messages.insert(self.seq, (relay_message.clone().into(), maybe_ack_tx));
                    self.last_message = relay_message.clone().into();
                    sender_tx.send(relay_message.into()).await.wrap_err("Failed to send outgoing message")?;
                }
                Some(command) = command_rx.recv() => {
                    match command {
                        Command::ReplayPendingMessages => {
                            self.replay_pending_messages(&sender_tx).await?;
                        }
                        Command::GetPendingMessages(reply_tx) => {
                            let _ = reply_tx.send(self.pending_messages.len());
                        }
                        Command::Reconnect => {
                            tracing::info!("Reconnecting...");
                            return Ok(());
                        }
                    }
                }
                _ = interval.tick() => {
                    self.seq = self.seq.wrapping_add(1);
                    sender_tx
                    .send(Heartbeat { seq: self.seq }.into())
                    .await
                    .wrap_err("Failed to send heartbeat")?;
                },
            }
        }
    }

    async fn replay_pending_messages(
        &mut self,
        sender_tx: &Sender<RelayConnectRequest>,
    ) -> Result<()> {
        if !self.pending_messages.is_empty() {
            tracing::warn!("Replaying pending messages: {:?}", self.pending_messages);
            for (_key, (msg, sender)) in self.pending_messages.iter_mut() {
                sender_tx
                    .send(msg.clone())
                    .await
                    .wrap_err("Failed to send pending message")?;
                // If there's a sender, send a signal and set it to None. We are coming from a reconnect or a manual
                // retry, so we don't care about the acks.
                if let Some(tx) = sender.take() {
                    let _ = tx.send(());
                }
            }
        }
        Ok(())
    }

    async fn connect(
        &self,
    ) -> Result<(Streaming<RelayConnectResponse>, Sender<RelayConnectRequest>)> {
        let channel = Channel::from_shared(self.config.url.clone())?
            .tls_config(Self::create_tls_config())?
            .keep_alive_while_idle(true)
            .http2_keep_alive_interval(self.config.keep_alive_interval)
            .keep_alive_timeout(self.config.keep_alive_timeout)
            .connect_timeout(self.config.connect_timeout)
            .timeout(self.config.request_timeout)
            .connect()
            .await
            .wrap_err("Failed to create gRPC channel")?;

        // TODO: Make the buffer size configurable
        let (sender_tx, sender_rx) = mpsc::channel(32);

        let mut client = RelayServiceClient::new(channel);
        let response = client.relay_connect(ReceiverStream::new(sender_rx));
        self.send_connect_request(&sender_tx).await?;
        let mut response_stream = response.await?.into_inner();

        self.wait_for_connect_response(&mut response_stream).await?;
        Ok((response_stream, sender_tx))
    }

    // TODO: See if we can move this setup into `orb-security-utils`.
    fn create_tls_config() -> ClientTlsConfig {
        ClientTlsConfig::new().ca_certificates(vec![
            Certificate::from_pem(AWS_ROOT_CA1_CERT),
            Certificate::from_pem(AWS_ROOT_CA2_CERT),
            Certificate::from_pem(AWS_ROOT_CA3_CERT),
            Certificate::from_pem(AWS_ROOT_CA4_CERT),
            Certificate::from_pem(SFS_ROOT_G2_CERT),
            Certificate::from_pem(GTS_ROOT_R1_CERT),
            Certificate::from_pem(GTS_ROOT_R2_CERT),
            Certificate::from_pem(GTS_ROOT_R3_CERT),
            Certificate::from_pem(GTS_ROOT_R4_CERT),
        ])
    }

    async fn send_connect_request(
        &self,
        tx: &mpsc::Sender<RelayConnectRequest>,
    ) -> Result<()> {
        tx.send(RelayConnectRequest {
            msg: Some(relay_connect_request::Msg::ConnectRequest(ConnectRequest {
                client_id: Some(Entity {
                    id: self.config.src_id.clone(),
                    entity_type: match self.config.mode {
                        Mode::Orb => EntityType::Orb as i32,
                        Mode::App => EntityType::App as i32,
                    },
                }),
                auth_method: Some(match &self.config.auth {
                    Auth::Token(t) => AuthMethod::Token(t.token.clone()),
                    Auth::ZKP(z) => AuthMethod::ZkpAuthRequest(ZkpAuthRequest {
                        root: z.root.clone(),
                        signal: z.signal.clone(),
                        nullifier_hash: z.nullifier_hash.clone(),
                        proof: z.proof.clone(),
                    }),
                }),
            })),
        })
        .await
        .wrap_err("Failed to send connect request")
    }

    async fn wait_for_connect_response(
        &self,
        response_stream: &mut Streaming<RelayConnectResponse>,
    ) -> Result<()> {
        while let Some(message) = response_stream.next().await {
            let message = message?.msg.ok_or_eyre("ConnectResponse msg is missing")?;
            if let relay_connect_response::Msg::ConnectResponse(ConnectResponse {
                success,
                error,
                ..
            }) = message
            {
                return if success {
                    tracing::info!("Successful connection");
                    Ok(())
                } else {
                    Err(eyre::eyre!("Failed to establish connection: {error:?}"))
                };
            }
        }
        Err(eyre::eyre!(
            "Connection stream ended before receiving ConnectResponse"
        ))
    }

    async fn handle_message(
        &self,
        payload: RelayPayload,
        message_buffer: &Arc<Mutex<VecDeque<RelayPayload>>>,
    ) -> Result<()> {
        let mut buffer = message_buffer.lock().await;
        if buffer.len() >= self.config.max_buffer_size {
            // Remove the oldest message to maintain the buffer size
            let msg: Vec<RelayPayload> = buffer.drain(0..1).collect();
            tracing::warn!("Buffer is full, removing oldest message: {msg:?}");
        }
        buffer.push_back(payload);
        Ok(())
    }
}
