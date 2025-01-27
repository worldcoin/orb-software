use eyre::WrapErr;
use orb_update_agent_core::ManifestComponent;
use orb_update_agent_dbus::{
    ComponentState, ComponentStatus, UpdateAgentManager, UpdateAgentManagerT,
};
use zbus::blocking::object_server::InterfaceRef;

#[derive(Debug, Clone, Default)]
pub struct UpdateProgress {
    pub components: Vec<ComponentStatus>,
}

impl UpdateAgentManagerT for UpdateProgress {
    fn progress(&self) -> Vec<ComponentStatus> {
        self.components.clone()
    }
}

pub fn init_dbus_properties(
    components: &[ManifestComponent],
    iface: &InterfaceRef<UpdateAgentManager<UpdateProgress>>,
) {
    iface.get_mut().0.components = components
        .iter()
        .map(|c| ComponentStatus {
            name: c.name.clone(),
            state: ComponentState::None,
            progress: 0,
        })
        .collect();
}

pub fn update_dbus_properties(
    name: &str,
    state: ComponentState,
    progress: u8,
    iface: &InterfaceRef<UpdateAgentManager<UpdateProgress>>,
) -> eyre::Result<()> {
    if let Some(component) = iface
        .get_mut()
        .0
        .components
        .iter_mut()
        .find(|c| c.name == name)
    {
        component.state = state;
        component.progress = progress;
    }
    zbus::block_on(iface.get_mut().progress_changed(iface.signal_context()))
        .wrap_err("Failed to emit progress_changed signal")?;

    Ok(())
}
