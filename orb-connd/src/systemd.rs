use color_eyre::Result;
use zbus_systemd::systemd1::{ManagerProxy, ServiceProxy};

#[derive(Clone)]
pub struct Systemd {
    system_bus: zbus::Connection,
}

impl Systemd {
    pub fn new(system_bus: zbus::Connection) -> Self {
        Self { system_bus }
    }

    pub async fn restart_service(&self, unit: &str) -> Result<()> {
        let manager = ManagerProxy::new(&self.system_bus).await?;
        let _ = manager
            .restart_unit(unit.to_string(), "replace".to_string())
            .await?;

        Ok(())
    }

    pub async fn loaded_services(&self) -> Result<Vec<(String, ServiceProxy<'_>)>> {
        let manager = ManagerProxy::new(&self.system_bus).await?;

        let units = manager
            .list_units_by_patterns(Vec::new(), vec!["*.service".to_string()])
            .await?;

        let mut services = Vec::with_capacity(units.len());

        for (
            name,
            _description,
            _load_state,
            _active_state,
            _sub_state,
            _following_unit,
            object_path,
            _job_id,
            _job_type,
            _job_path,
        ) in units
        {
            let service = ServiceProxy::builder(&self.system_bus)
                .destination("org.freedesktop.systemd1")?
                .path(object_path)?
                .build()
                .await?;

            services.push((name, service));
        }

        Ok(services)
    }
}

pub struct IpAccounting {
    pub ingress_bytes: u64,
    pub ingress_packets: u64,
    pub egress_bytes: u64,
    pub egress_packets: u64,
}

#[allow(async_fn_in_trait)]
pub trait ServiceProxyExt {
    async fn get_ip_accounting(&self) -> Result<Option<IpAccounting>>;
}

impl ServiceProxyExt for ServiceProxy<'_> {
    async fn get_ip_accounting(&self) -> Result<Option<IpAccounting>> {
        if !self.ip_accounting().await? {
            return Ok(None);
        }

        Ok(Some(IpAccounting {
            ingress_bytes: self.ip_ingress_bytes().await?,
            egress_bytes: self.ip_egress_bytes().await?,
            ingress_packets: self.ip_ingress_packets().await?,
            egress_packets: self.ip_egress_packets().await?,
        }))
    }
}
