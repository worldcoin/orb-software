use orb_update_agent_core::ManifestComponent;
use orb_update_agent_dbus::{
    ComponentState, ComponentStatus, UpdateAgentManager, UpdateAgentManagerT,
};
use tracing::warn;
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
        })
        .collect();
}

pub fn update_dbus_properties(
    name: &str,
    state: ComponentState,
    iface: &InterfaceRef<UpdateAgentManager<UpdateProgress>>,
) {
    iface
        .get_mut()
        .0
        .components
        .iter_mut()
        .find(|c| c.name == name)
        .map(|component| {
            component.state = state;
            if let Err(err) = async_io::block_on(
                iface.get_mut().progress_changed(iface.signal_context()),
            ) {
                warn!("Failed to emit signal on dbus: {err:?}")
            }
        })
        .unwrap_or_else(|| {
            warn!("failed updating dbus property: {name}");
        });
}
