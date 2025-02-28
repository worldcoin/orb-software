use color_eyre::eyre::Result;
use orb_relay_client::{Client, SendMessage};
use orb_relay_messages::{
    fleet_cmdr::v1::{JobExecution, JobExecutionUpdate, JobNotify, JobRequestNext},
    prost::{Message, Name},
    prost_types::Any,
    relay::entity::EntityType,
};
use tracing::{error, info};

#[derive(Debug, Clone)]
pub struct JobClient {
    relay_client: Client,
    fleet_cmdr_id: String,
    relay_namespace: String,
}

impl JobClient {
    pub fn new(
        relay_client: Client,
        fleet_cmdr_id: &str,
        relay_namespace: &str,
    ) -> Self {
        Self {
            relay_client,
            fleet_cmdr_id: fleet_cmdr_id.to_string(),
            relay_namespace: relay_namespace.to_string(),
        }
    }

    pub async fn listen_for_job(&self) -> Result<JobExecution, orb_relay_client::Err> {
        loop {
            match self.relay_client.recv().await {
                Ok(msg) => {
                    let any = match Any::decode(msg.payload.as_slice()) {
                        Ok(any) => any,
                        Err(e) => {
                            error!("error decoding message: {:?}", e);
                            continue;
                        }
                    };
                    if any.type_url == JobNotify::type_url() {
                        match JobNotify::decode(any.value.as_slice()) {
                            Ok(job_notify) => {
                                info!("received JobNotify: {:?}", job_notify);
                                let _ = self.request_next_job().await;
                            }
                            Err(e) => {
                                error!("error decoding JobNotify: {:?}", e);
                            }
                        }
                    } else if any.type_url == JobExecution::type_url() {
                        match JobExecution::decode(any.value.as_slice()) {
                            Ok(job) => {
                                info!("received JobExecution: {:?}", job);
                                return Ok(job);
                            }
                            Err(e) => {
                                error!("error decoding JobExecution: {:?}", e);
                            }
                        }
                    } else {
                        error!("received unexpected message type: {:?}", any.type_url);
                    }
                }
                Err(e) => {
                    error!("error receiving from relay: {:?}", e);
                    return Err(e);
                }
            }
        }
    }

    pub async fn request_next_job(&self) -> Result<(), orb_relay_client::Err> {
        let any = Any::from_msg(&JobRequestNext::default()).unwrap();
        match self
            .relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(self.fleet_cmdr_id.clone())
                    .namespace(self.relay_namespace.clone())
                    .payload(any.encode_to_vec()),
            )
            .await
        {
            Ok(_) => {
                info!("sent JobRequestNext");
                Ok(())
            }
            Err(e) => {
                error!("error sending JobRequestNext: {:?}", e);
                Err(e)
            }
        }
    }

    pub async fn send_job_update(
        &self,
        job_update: &JobExecutionUpdate,
    ) -> Result<(), orb_relay_client::Err> {
        info!("sending job update: {:?}", job_update);
        let any = Any::from_msg(job_update).unwrap();
        match self
            .relay_client
            .send(
                SendMessage::to(EntityType::Service)
                    .id(self.fleet_cmdr_id.clone())
                    .namespace(self.relay_namespace.clone())
                    .payload(any.encode_to_vec()),
            )
            .await
        {
            Ok(_) => {
                info!("sent JobExecutionUpdate");
                Ok(())
            }
            Err(e) => {
                error!("error sending JobExecutionUpdate: {:?}", e);
                Err(e)
            }
        }
    }
}
