use std::path::PathBuf;

use color_eyre::{
    eyre::{eyre, WrapErr},
    Result,
};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tracing::{info, instrument};
use zenorb::{zenoh::query::Query, zoci::ZociQueryExt, Zenorb};

pub const GONDOR_BIN: &str = "/usr/local/bin/gondor-calls-for-ota";

pub async fn spawn_zoci_receiver(
    zenorb: &Zenorb,
    gondor_bin: PathBuf,
) -> Result<Vec<JoinHandle<()>>> {
    zenorb
        .receiver(gondor_bin)
        .queryable("job/gondor-calls-for-ota", gondor_calls_for_ota)
        .run()
        .await
}

#[instrument(skip(query))]
async fn gondor_calls_for_ota(gondor_bin: PathBuf, query: Query) -> Result<()> {
    let response = async {
        let version = query.payload_str()?;
        let version = version.trim();

        if version.is_empty() {
            return Err(eyre!("missing target version"));
        }

        info!("running {} {version}", gondor_bin.display());

        let output = Command::new(&gondor_bin)
            .arg(version)
            .output()
            .await
            .wrap_err("failed to spawn gondor")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(eyre!("gondor-calls-for-ota failed: {stderr}"));
        }

        Ok::<_, color_eyre::Report>(())
    }
    .await
    .map_err(|e| e.to_string());

    query.res(response).await?;

    Ok(())
}
