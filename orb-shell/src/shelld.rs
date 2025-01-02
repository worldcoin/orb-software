use crate::{
    cfg::Cfg,
    relay::{
        self,
        client::{self, RelayClient},
    },
    sshd::{self, Sshd},
    ClientId, ShellMsg,
};
use async_trait::async_trait;
use speare::*;
use std::{
    collections::{hash_map::Entry, HashMap},
    time::Duration,
};
use tracing::{debug, error, info};

pub struct ShellDaemon {
    relay_client: Handle<client::Msg>,
    sshd_map: HashMap<ClientId, Handle<sshd::Msg>>,
}

#[async_trait]
impl Actor for ShellDaemon {
    type Props = ();
    type Msg = ShellMsg;
    type Err = relay::Err;

    async fn init(ctx: &mut Ctx<Self>) -> Result<Self, Self::Err> {
        info!("Starting ShellDaemon");
        let relay_client = ctx.spawn::<RelayClient>(client::Props {
            cfg: Cfg::from_env()?,
            shell: ctx.this().clone(),
        });

        Ok(ShellDaemon {
            relay_client,
            sshd_map: HashMap::default(),
        })
    }

    async fn exit(_: Option<Self>, reason: ExitReason<Self>, _: &mut Ctx<Self>) {
        error!("ShellDaemon exited. Reason: {reason:?}");
    }

    async fn handle(
        &mut self,
        msg: Self::Msg,
        ctx: &mut Ctx<Self>,
    ) -> Result<(), Self::Err> {
        use ShellMsg::*;
        match msg {
            SshdClosed(client_id) => {
                self.sshd_map.remove(&client_id);
            }

            FromSsh(client_id, payload, seq) => {
                let sshd_instance = match self.sshd_map.entry(client_id.clone()) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        if seq > 0 {
                            debug!("Out of order message for client {client_id}. Expected 0 got {seq}");
                            return Ok(());
                        }

                        let handle = ctx.spawn::<Sshd>(sshd::Props {
                            client_id,
                            shelld: ctx.this().clone(),
                            relay_client: self.relay_client.clone(),
                            max_idle: Duration::from_secs(300),
                        });

                        entry.insert(handle)
                    }
                };

                sshd_instance.send(sshd::Msg::Stdin(payload))
            }
        }

        Ok(())
    }

    fn supervision(_: &Self::Props) -> Supervision {
        Supervision::one_for_one()
            .directive(Directive::Restart)
            .backoff(Backoff::Static(Duration::from_secs(5)))
            .when(|_: &sshd::Err| Directive::Stop)
    }
}
