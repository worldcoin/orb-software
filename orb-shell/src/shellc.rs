use crate::{
    cfg::Cfg,
    relay::client::{self, RelayClient},
    ShellMsg, BUFFER_SIZE,
};
use async_trait::async_trait;
use color_eyre::eyre::{self};
use speare::*;
use tokio::io::{stdout, AsyncWriteExt};
use tracing::debug;
use uuid::Uuid;

pub struct ShellClient;

#[async_trait]
impl Actor for ShellClient {
    type Props = ();
    type Msg = ShellMsg;
    type Err = eyre::Error;

    async fn init(ctx: &mut Ctx<Self>) -> Result<Self, Self::Err> {
        debug!("Starting ShellClient");
        let uuid = Uuid::new_v4();

        let mut cfg = Cfg {
            client_id: "420".to_string(),
            auth_token: "".to_string(),
            domain: "relay.stage.orb.worldcoin.org".to_string(),
        };

        let target_client_id = "orb-shell-420".to_string();
        cfg.client_id = format!("{}-{}", cfg.client_id, uuid);

        let relay_client = ctx.spawn::<RelayClient>(client::Props {
            cfg,
            shell: ctx.this().clone(),
        });

        let relayc = relay_client.clone();

        ctx.subtask_blocking(move || {
            let mut stdin = std::io::stdin();
            let mut buffer = [0; BUFFER_SIZE];
            while let Ok(n) = std::io::Read::read(&mut stdin, &mut buffer) {
                if n == 0 {
                    break;
                }

                relayc.send(client::Msg::Send(
                    target_client_id.clone(),
                    buffer[..n].to_vec(),
                ))
            }

            Ok(())
        });

        Ok(ShellClient)
    }

    async fn handle(
        &mut self,
        msg: Self::Msg,
        _: &mut Ctx<Self>,
    ) -> Result<(), Self::Err> {
        if let ShellMsg::FromSsh(_client_id, payload, _seq) = msg {
            let mut stdout = stdout();
            stdout.write_all(&payload).await?;
            stdout.flush().await?;
        }

        Ok(())
    }
}
