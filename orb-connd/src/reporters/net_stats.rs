use color_eyre::{
    eyre::{Context, ContextCompat},
    Result,
};
use oes::NetStats;
use speare::mini;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{fs, time};
use tracing::{info, warn};

pub struct Args {
    pub poll_interval: Duration,
    pub sysfs: PathBuf,
    pub zsender: zenorb::Sender,
}

pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting netstats reporter");
    let mut update_interval = time::interval(ctx.poll_interval);

    loop {
        update_interval.tick().await;

        let ifaces = iface_paths(&ctx.sysfs)
            .await
            .inspect_err(|e| warn!("failed reading network ifaces from sysfs: {e}"))?;

        let mut all_stats = Vec::new();

        for iface_path in ifaces {
            match collect(&iface_path).await {
                Err(e) => {
                    warn!("faield to collectn netstats on {iface_path:?}, err: {e:?}")
                }

                Ok(stats) => {
                    all_stats.push(stats);
                }
            }
        }

        let payload = serde_json::to_string(&all_stats)
            .wrap_err("failed to serialze netstats")?;

        let _ = ctx.publish("netstats", all_stats.clone());

        let _ = ctx
            .zsender
            .publisher("oes/netstats")?
            .put(payload)
            .attachment(oes::Headers::default().mode(oes::Mode::CacheOnly))
            .await
            .inspect_err(|e| {
                warn!("failed to publish oes/netstats on zenoh, err: {e:?}",)
            });
    }
}

async fn iface_paths(sysfs: &Path) -> Result<Vec<PathBuf>> {
    let ifaces_dir = sysfs.join("class").join("net");
    let mut dir = fs::read_dir(ifaces_dir).await?;

    let mut paths = Vec::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();

        let file = path
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or_default();

        if file.starts_with("eth")
            || file.starts_with("wwan")
            || file.starts_with("wlan")
        {
            paths.push(path)
        }
    }

    Ok(paths)
}

async fn collect(iface_path: &PathBuf) -> Result<NetStats> {
    let iface = iface_path
        .file_name()
        .and_then(|f| f.to_str())
        .wrap_err_with(|| format!("err reading iface name from {iface_path:?}"))?;

    let stats_path = iface_path.join("statistics");
    let tx_path = stats_path.join("tx_bytes");
    let rx_path = stats_path.join("rx_bytes");

    let tx_bytes = String::from_utf8_lossy(&fs::read(tx_path).await?)
        .trim()
        .parse()?;

    let rx_bytes = String::from_utf8_lossy(&fs::read(rx_path).await?)
        .trim()
        .parse()?;

    Ok(NetStats {
        iface: iface.into(),
        tx_bytes,
        rx_bytes,
    })
}
