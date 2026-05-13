use crate::{
    statsd::StatsdClient,
    systemd::{IpAccounting, ServiceProxyExt, Systemd},
};
use color_eyre::Result;
use speare::mini;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::time;
use tracing::{info, warn};

pub struct Args {
    pub statsd: Arc<dyn StatsdClient>,
    pub systemd: Systemd,
}

pub async fn report(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting data usage reporter");

    let mut data_usage_map: HashMap<String, IpAccounting> = HashMap::new();

    loop {
        let new_data_usage_map = get_data_usage_map(&ctx.systemd).await?;

        for (unit, usage) in new_data_usage_map.iter() {
            let Some(old_usage) = data_usage_map.get(unit) else {
                continue;
            };

            let ingress_diff = usage.ingress_bytes - old_usage.ingress_bytes;
            let egress_diff = usage.egress_bytes - old_usage.egress_bytes;
            warn!("unit: {unit}\ningress:{ingress_diff}\negress:{egress_diff}");

            if ingress_diff > 0 {
                ctx.statsd
                    .count(
                        "orb.platformm.connd.service_ingress_bytes",
                        ingress_diff as i64,
                        Vec::new(),
                    )
                    .await?;
            }

            if egress_diff > 0 {
                ctx.statsd
                    .count(
                        "orb.platformm.connd.service_egress_bytes",
                        ingress_diff as i64,
                        Vec::new(),
                    )
                    .await?;
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
