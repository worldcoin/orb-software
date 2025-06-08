use eyre::WrapErr;
use orb_update_agent_core::ManifestComponent;
use orb_update_agent_dbus::{
    ComponentState, ComponentStatus, UpdateAgentManager, UpdateAgentManagerT,
    UpdateAgentState,
};
use zbus::blocking::object_server::InterfaceRef;

#[derive(Debug, Clone, Default)]
pub struct UpdateProgress {
    pub components: Vec<ComponentStatus>,
    pub overall_status: UpdateAgentState,
}

impl UpdateProgress {
    pub fn set_overall_status(&mut self, status: UpdateAgentState) {
        self.overall_status = status;
    }
}

impl UpdateAgentManagerT for UpdateProgress {
    fn progress(&self) -> Vec<ComponentStatus> {
        self.components.clone()
    }

    fn overall_status(&self) -> UpdateAgentState {
        self.overall_status
    }

    fn overall_progress(&self) -> u8 {
        // TODO do a weighted average based on actual size of the components
        if self.components.is_empty() {
            return 0;
        }

        let total_progress: u32 =
            self.components.iter().map(|c| c.progress as u32).sum();

        (total_progress / self.components.len() as u32) as u8
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

pub fn update_dbus_progress(
    component_update: Option<ComponentStatus>,
    overall_status: Option<UpdateAgentState>,
    iface: &InterfaceRef<UpdateAgentManager<UpdateProgress>>,
) -> eyre::Result<()> {
    if let Some(update) = component_update {
        if let Some(component) = iface
            .get_mut()
            .0
            .components
            .iter_mut()
            .find(|c| c.name == update.name)
        {
            component.state = update.state;
            component.progress = update.progress;
        }
    }

    if let Some(status) = overall_status {
        iface.get_mut().0.set_overall_status(status);
    }

    zbus::block_on(iface.get_mut().progress_changed(iface.signal_context()))
        .wrap_err("Failed to emit progress_changed signal")?;
    zbus::block_on(
        iface
            .get_mut()
            .overall_status_changed(iface.signal_context()),
    )
    .wrap_err("Failed to emit overall_status_changed signal")?;
    zbus::block_on(
        iface
            .get_mut()
            .overall_progress_changed(iface.signal_context()),
    )
    .wrap_err("Failed to emit overall_progress_changed signal")?;

    Ok(())
}
