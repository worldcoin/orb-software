use crate::cfg::Cfg;
use crate::{relay, ClientId, ShellMsg};
use async_trait::async_trait;
use color_eyre::eyre::{eyre, Context};
use orb_relay_messages::{
    prost_types::Any,
    relay::{
        connect_request::AuthMethod,
        entity::EntityType,
        relay_connect_request::{self},
        relay_connect_response,
        relay_service_client::RelayServiceClient,
        ConnectRequest, ConnectResponse, Entity, RelayConnectRequest,
        RelayConnectResponse, RelayPayload,
    },
};
use speare::*;
use std::time::Duration;
use tokio::{sync::mpsc, time};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tonic::{
    transport::{ClientTlsConfig, Endpoint},
    Streaming,
};
use tracing::{debug, error};

const NAMESPACE: &str = "orb-shell";

pub struct RelayClient {
    client_id: String,
    entity_type: EntityType,
    tx: mpsc::Sender<RelayConnectRequest>,
    seq: u64,
}

pub enum Msg {
    Send(ClientId, Vec<u8>),
}

pub struct Props {
    pub cfg: Cfg,
    pub shell: Handle<ShellMsg>,
}

#[async_trait]
impl Actor for RelayClient {
    type Props = Props;
    type Msg = Msg;
    type Err = relay::Err;

    async fn init(ctx: &mut Ctx<Self>) -> Result<Self, Self::Err> {
        let cfg = &ctx.props().cfg;
        let client_id = cfg.client_id.clone();
        let client_id = format!("orb-shell-{}", client_id);
        let entity_type = EntityType::Orb;

        debug!("Starting Orb Relay client with id: {client_id}");

        let tls_config = ClientTlsConfig::new().with_native_roots();

        let channel = Endpoint::from_shared(format!("https://{}", cfg.domain))?
            .tls_config(tls_config)?
            .keep_alive_while_idle(true)
            .connect()
            .await?;

        let mut relay_client = RelayServiceClient::new(channel);
        let (tx, rx) = mpsc::channel(4);

        tx.send(RelayConnectRequest {
            msg: Some(relay_connect_request::Msg::ConnectRequest(ConnectRequest {
                client_id: Some(Entity {
                    id: client_id.to_owned(),
                    entity_type: entity_type as i32,
                    namespace: NAMESPACE.to_owned(),
                }),
                auth_method: Some(AuthMethod::Token(cfg.auth_token.to_owned())),
            })),
        })
        .await
        .wrap_err("Failed to send RelayConnectRequest")?;

        let mut response_stream: Streaming<RelayConnectResponse> = relay_client
            .relay_connect(ReceiverStream::new(rx))
            .await?
            .into_inner();

        let connect_res = async {
            while let Some(message) = response_stream.next().await {
                match message?.msg {
                    Some(relay_connect_response::Msg::ConnectResponse(
                        ConnectResponse { success: true, .. },
                    )) => {
                        debug!("Connection established successfully.");
                        break;
                    }

                    Some(relay_connect_response::Msg::ConnectResponse(
                        ConnectResponse { success: false, .. },
                    )) => {
                        debug!("Failed to establish connection.");
                        return Err(eyre!("Failed to establish connection."));
                    }

                    Some(other_msg) => {
                        debug!(" Received unexpected message: {:?}", other_msg);
                    }

                    None => (),
                }
            }

            Ok(())
        };

        time::timeout(Duration::from_secs(5), connect_res)
            .await
            .wrap_err("Timed out trying to establish a connection")??;

        // Reading from sever subtask
        let shell = ctx.props().shell.clone();
        ctx.subtask(async move {
            while let Some(message) = response_stream.next().await {
                match message?.msg {
                    Some(relay_connect_response::Msg::Payload(RelayPayload {
                        payload: Some(any),
                        src: Some(Entity { id, namespace, .. }),
                        seq,
                        ..
                    })) if namespace == NAMESPACE => {
                        shell.send((id, any.value, seq));
                    }

                    Some(other_msg) => {
                        debug!(" Received a non-Any message: {:?}", other_msg)
                    }

                    None => (),
                }
            }

            Ok(())
        });

        Ok(RelayClient {
            client_id: client_id.to_owned(),
            entity_type,
            tx,
            seq: 0,
        })
    }

    async fn exit(_: Option<Self>, reason: ExitReason<Self>, _: &mut Ctx<Self>) {
        error!("RelayClient exiting: {reason:?}");
    }

    async fn handle(
        &mut self,
        msg: Self::Msg,
        _: &mut Ctx<Self>,
    ) -> Result<(), Self::Err> {
        let Msg::Send(dst_client_id, bytes) = msg;
        debug!("Sending {} bytes to {}", bytes.len(), dst_client_id);

        let src = Some(Entity {
            id: self.client_id.clone(),
            entity_type: self.entity_type as i32,
            namespace: NAMESPACE.to_owned(),
        });

        let dst = Some(Entity {
            id: dst_client_id,
            entity_type: self.entity_type as i32,
            namespace: NAMESPACE.to_owned(),
        });

        let payload = Some(Any {
            type_url: "".to_string(),
            value: bytes,
        });

        self.tx
            .send(RelayConnectRequest {
                msg: Some(relay_connect_request::Msg::Payload(RelayPayload {
                    src,
                    dst,
                    seq: self.seq,
                    payload,
                })),
            })
            .await
            .wrap_err("Failed to send message to Orb Relay Server")?;

        self.seq = self.seq.wrapping_add(1);

        Ok(())
    }
}
