//! Serves the current monitoring token to authorized Unix-socket peers.
//!
//! The socket boundary filters tokens against the supplied clock so an expired
//! token is never exposed, even when it expires after server startup.

use crate::server::{Clock, StoredToken};
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use speare::mini::{self};
use std::{io::ErrorKind, ops::Deref, path::PathBuf};
use tokio::{fs, io::AsyncWriteExt, net::UnixListener};
use tracing::warn;

pub struct Args {
    pub token: StoredToken,
    pub socket_path: PathBuf,
    pub dd_agent_uid: u32,
    pub clock: Clock,
}

pub async fn task(ctx: mini::Ctx<Args>) -> Result<()> {
    match fs::remove_file(&ctx.socket_path).await {
        Err(e) if e.kind() == ErrorKind::NotFound => (),
        Err(e) => Err(e).wrap_err("failed to delete pre-existing socket")?,
        Ok(_) => (),
    }

    let listener = UnixListener::bind(&ctx.socket_path)?;

    loop {
        let (mut stream, _) = listener.accept().await?;
        let uid = stream.peer_cred().map(|cred| cred.uid())?;

        if uid != ctx.dd_agent_uid {
            warn!("unauthorized uid {uid} tried to connect. ignoring");
            continue;
        }

        let token = ctx.token.clone();
        let clock = ctx.clock.clone();

        ctx.oneshot(async move |_| -> Result<()> {
            let token = token
                .read()
                .map_err(|e| eyre!("{e:?}"))?
                .deref()
                .clone()
                .filter(|token| !token.is_expired_at(clock.now()));
            let payload = serde_json::to_vec(&token)?;
            stream.write_all(&payload).await?;

            Ok(())
        })?;
    }
}
