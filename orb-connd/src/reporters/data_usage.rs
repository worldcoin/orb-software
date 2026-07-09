use crate::systemd::{IpAccounting, ServiceProxyExt, Systemd};
use color_eyre::Result;
use crabwire::inject;
use orb_dogd::MetricEmitter;
use speare::mini;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::time;
use tracing::{info, warn};

pub struct Args<M: MetricEmitter> {
    pub statsd: Arc<M>,
}

#[inject(systemd: &Systemd)]
pub async fn report(ctx: mini::Ctx<Args<impl MetricEmitter>>) -> Result<()> {
    info!("starting data usage reporter");

    let mut data_usage_map: HashMap<String, IpAccounting> = HashMap::new();

    loop {
        let new_data_usage_map = get_data_usage_map(systemd).await?;

        for (unit, usage) in new_data_usage_map.iter() {
            let Some(old_usage) = data_usage_map.get(unit) else {
                continue;
            };

            if usage.ingress_bytes < old_usage.ingress_bytes {
                warn!(
                    "unit: {unit} ingress counter reset: {} -> {}",
                    old_usage.ingress_bytes, usage.ingress_bytes
                );
            }

            if usage.egress_bytes < old_usage.egress_bytes {
                warn!(
                    "unit: {unit} egress counter reset: {} -> {}",
                    old_usage.egress_bytes, usage.egress_bytes
                );
            }

            let ingress_diff =
                usage.ingress_bytes.saturating_sub(old_usage.ingress_bytes);
            let egress_diff = usage.egress_bytes.saturating_sub(old_usage.egress_bytes);
            let tags = vec![format!("service:{unit}")];

            if ingress_diff > 0 {
                let _ = ctx.statsd.count(
                    "orb.platform.connd.service_ingress_bytes",
                    ingress_diff as i64,
                    tags.clone(),
                );
            }

            if egress_diff > 0 {
                let _ = ctx.statsd.count(
                    "orb.platform.connd.service_egress_bytes",
                    egress_diff as i64,
                    tags,
                );
            }
        }

        data_usage_map = new_data_usage_map;
        time::sleep(Duration::from_secs(30)).await;
    }
}

async fn get_data_usage_map(
    systemd: &Systemd,
) -> Result<HashMap<String, IpAccounting>> {
    let mut ip_accountings = HashMap::new();

    for (unit, service) in systemd.loaded_services().await? {
        if let Some(ip_accounting) = service.get_ip_accounting().await? {
            ip_accountings.insert(unit, ip_accounting);
        }
    }

    Ok(ip_accountings)
}
