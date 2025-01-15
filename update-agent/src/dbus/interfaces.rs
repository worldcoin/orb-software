use orb_update_agent_core::ManifestComponent;
use orb_update_agent_dbus::{
    ComponentState, ComponentStatus, UpdateProgress, UpdateProgressT,
};
use tracing::warn;
use zbus::blocking::object_server::InterfaceRef;

/// Represents the overall update status.
///
/// Provides detailed state of the update progress for each component.
/// In case of failure, the error message is stored in the `error` field.
#[derive(Debug, Clone)]
pub struct UpdateStatus {
    pub components: zbus::fdo::Result<Vec<ComponentStatus>>,
}

impl UpdateProgressT for UpdateStatus {
    fn status(&self) -> zbus::fdo::Result<Vec<ComponentStatus>> {
        match &self.components {
            Ok(components) => Ok(components.to_owned()),
            Err(e) => Err(e.to_owned()),
        }
    }
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            components: Ok(Vec::default()),
        }
    }
}

pub fn init_dbus_properties(
    components: &[ManifestComponent],
    iref: &InterfaceRef<UpdateProgress<UpdateStatus>>,
) {
    iref.get_mut().0.components = Ok(components
        .iter()
        .map(|c| ComponentStatus {
            name: c.name.clone(),
            state: ComponentState::None,
        })
        .collect());
}

pub fn update_dbus_properties(
    name: &str,
    state: ComponentState,
    iref: &InterfaceRef<UpdateProgress<UpdateStatus>>,
) {
    let _ = iref.get_mut().0.components.as_mut().map(|components| {
        components
            .iter_mut()
            .find(|c| c.name == name)
            .map(|component| {
                component.state = state;
                if let Err(err) = async_io::block_on(
                    iref.get_mut().status_changed(iref.signal_context()),
                ) {
                    warn!("Failed to emit signal on dbus: {err:?}")
                }
            })
            .unwrap_or_else(|| {
                warn!("failed updating dbus property: {name}");
            });
    });
}
