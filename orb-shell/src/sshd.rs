use crate::{
    relay::{self, client},
    ClientId, ShellMsg, BUFFER_SIZE,
};
use async_trait::async_trait;
use color_eyre::eyre::{self, eyre, Context, ContextCompat};
use derive_more::From;
use speare::*;
use std::{
    io::{Read, Write},
    process::{self, Command, Stdio},
    time::Duration,
};
use tokio::{task, time::Instant};
use tracing::{info, warn};

pub struct Sshd {
    sshd: process::Child,
    last_write: Instant,
}

pub enum Msg {
    Stdin(Vec<u8>),
    CheckIdle,
}

#[derive(From, Debug)]
pub enum Err {
    ReachedMaxIdleTime,
    Other(eyre::Error),
}

pub struct Props {
    /// The client for which this sshd instance
    /// was created.
    pub client_id: ClientId,
    pub shelld: Handle<ShellMsg>,
    pub relay_client: Handle<relay::client::Msg>,
    /// Maximum amount of time sshd instace can go idle before
    /// being shut down.
    pub max_idle: Duration,
}

#[async_trait]
impl Actor for Sshd {
    type Props = Props;
    type Msg = Msg;
    type Err = Err;

    async fn init(ctx: &mut Ctx<Self>) -> Result<Self, Self::Err> {
        info!(
            "Starting sshd instance for client: {}",
            ctx.props().client_id
        );
        let mut sshd = Command::new("/usr/sbin/sshd")
            .arg("-i")
            .arg("-ddd")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                eyre!(
                    "Failed to start sshd for client {}. Error: '{}'",
                    ctx.props().client_id,
                    e
                )
            })?;

        let mut sshd_stdout = sshd.stdout.take().unwrap();
        let relay_client = ctx.props().relay_client.clone();
        let client_id = ctx.props().client_id.clone();

        ctx.subtask_blocking(move || {
            let mut buffer = [0; BUFFER_SIZE];
            while let Ok(n) = sshd_stdout.read(&mut buffer) {
                if n == 0 {
                    break;
                }

                relay_client
                    .send(client::Msg::Send(client_id.clone(), buffer[..n].to_vec()));
            }

            Ok(())
        });

        ctx.this().send_in(Msg::CheckIdle, ctx.props().max_idle);

        Ok(Sshd {
            sshd,
            last_write: Instant::now(),
        })
    }

    async fn exit(_: Option<Self>, reason: ExitReason<Self>, ctx: &mut Ctx<Self>) {
        let Props {
            client_id, shelld, ..
        } = ctx.props();

        warn!(
            "Exiting sshd for client {}. Reason: '{:?}'",
            client_id, reason
        );

        shelld.send(client_id.clone())
    }

    async fn handle(
        &mut self,
        msg: Self::Msg,
        ctx: &mut Ctx<Self>,
    ) -> Result<(), Self::Err> {
        match msg {
            Msg::Stdin(bytes) => {
                let mut stdin =
                    self.sshd.stdin.take().wrap_err("could not take stdin")?;

                let client_id = ctx.props().client_id.clone();
                let stdin = task::spawn_blocking(move || {
                    stdin.write_all(&bytes).map_err(|e| {
                        eyre!(
                            "Failed to write to stdin of client {}. Error: '{}'",
                            client_id,
                            e
                        )
                    })?;

                    Ok::<_, eyre::Error>(stdin)
                })
                .await
                .wrap_err("failed to write to stdin")??;

                self.sshd.stdin = Some(stdin);
                self.last_write = Instant::now();
            }

            Msg::CheckIdle => {
                if self.last_write.elapsed() > ctx.props().max_idle {
                    return Err(Err::ReachedMaxIdleTime);
                }

                let next_check = ctx.props().max_idle - self.last_write.elapsed();
                ctx.this().send_in(Msg::CheckIdle, next_check);
            }
        }

        Ok(())
    }
}
