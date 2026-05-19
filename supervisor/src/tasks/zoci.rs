use std::{borrow::Cow, path::PathBuf};

use color_eyre::{
    eyre::{eyre, WrapErr},
    Result,
};
use serde::Deserialize;
use tokio::process::Command;
use tokio::task::JoinHandle;
use tracing::{field, info, instrument, warn, Span};
use zenorb::{zenoh::query::Query, zoci::ZociQueryExt, Zenorb};

#[derive(Deserialize)]
struct GondorRequest {
    version: String,
    #[serde(default)]
    no_restart: bool,
}

pub const GONDOR_BIN: &str = "/usr/local/bin/gondor-calls-for-ota";

pub async fn spawn_zoci_receiver(
    zenorb: &Zenorb,
    gondor_bin: PathBuf,
) -> Result<Vec<JoinHandle<()>>> {
    zenorb
        .receiver(gondor_bin)
        .queryable("job/gondor", gondor)
        .run()
        .await
}

#[instrument(
    skip(query),
    fields(key_expr = %query.key_expr(), version = field::Empty, no_restart = field::Empty),
)]
async fn gondor(gondor_bin: PathBuf, query: Query) -> Result<()> {
    let response = async {
        let (version, no_restart): (Cow<'_, str>, bool) =
            match query.json::<GondorRequest>() {
                Ok(req) => (Cow::Owned(req.version), req.no_restart),
                Err(_) => (query.payload_str()?, false),
            };

        Span::current().record("version", version.as_ref());
        Span::current().record("no_restart", no_restart);

        info!(
            "received gondor query for version {version} (no_restart={no_restart}), running {}",
            gondor_bin.display()
        );

        let mut cmd = Command::new(&gondor_bin);
        cmd.arg(version.as_ref());
        if no_restart {
            cmd.arg("--no-restart");
        }
        let output = cmd.output().await.wrap_err("failed to spawn gondor")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(eyre!("gondor failed: {stderr}"));
        }

        info!("gondor succeeded for version {version}");
        Ok::<_, color_eyre::Report>(())
    }
    .await
    .map_err(|e| {
        warn!("gondor handler failed: {e}");
        e.to_string()
    });

    query.res(response).await?;

    Ok(())
}
