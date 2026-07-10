use std::time::Duration;

use async_trait::async_trait;
use color_eyre::{eyre::Context, Result};
use zbus_systemd::systemd1::{ManagerProxy, ServiceProxy, UnitProxy};

#[derive(Clone)]
pub struct SystemdDbus {
    system_bus: zbus::Connection,
}

impl SystemdDbus {
    pub fn new(system_bus: zbus::Connection) -> Self {
        Self { system_bus }
    }

    async fn is_service_active(&self, unit: &str) -> Result<bool> {
        let manager = ManagerProxy::new(&self.system_bus).await?;
        let path = manager.get_unit(unit.to_string()).await?;

        let unit_proxy = UnitProxy::builder(&self.system_bus)
            .destination("org.freedesktop.systemd1")?
            .path(path)?
            .build()
            .await?;
        Ok(unit_proxy.active_state().await? == "active")
    }
}

#[async_trait]
pub trait Systemd: 'static + Send + Sync {
    async fn restart_service(&self, unit: &str) -> Result<()>;

    async fn loaded_services(&self) -> Result<Vec<(String, ServiceProxy<'_>)>>;

    async fn wait_for_active(&self, unit: &str, timeout: Duration) -> Result<()>;
}

#[async_trait]
impl Systemd for SystemdDbus {
    async fn restart_service(&self, unit: &str) -> Result<()> {
        let manager = ManagerProxy::new(&self.system_bus).await?;
        let _ = manager
            .restart_unit(unit.to_string(), "replace".to_string())
            .await?;

        Ok(())
    }

    async fn loaded_services(&self) -> Result<Vec<(String, ServiceProxy<'_>)>> {
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

    async fn wait_for_active(&self, unit: &str, timeout: Duration) -> Result<()> {
        let is_active = async {
            loop {
                if self.is_service_active(unit).await.unwrap_or(false) {
                    return;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        };

        tokio::time::timeout(timeout, is_active)
            .await
            .with_context(|| format!("timed out waiting for {unit} to become active"))
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
