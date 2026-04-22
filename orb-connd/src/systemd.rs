use color_eyre::Result;
use zbus_systemd::systemd1::ManagerProxy;

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
}
