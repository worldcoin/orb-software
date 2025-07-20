use eyre::WrapErr;
use orb_update_agent_core::Claim;
use orb_update_agent_dbus::{
    ComponentState, ComponentStatus, UpdateAgentManager, UpdateAgentManagerT,
    UpdateAgentState,
};
use std::collections::HashMap;
use zbus::blocking::object_server::InterfaceRef;

#[derive(Debug, Clone, Default)]
pub struct UpdateProgress {
    pub components: Vec<ComponentStatus>,
    pub overall_status: UpdateAgentState,
    pub component_download_sizes: HashMap<String, u64>, // component_name -> download_size
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
        if self.components.is_empty() {
            return 0;
        }

        let total_download_size: u64 = self.component_download_sizes.values().sum();
        if total_download_size == 0 {
            // Fallback to simple average if no size information available
            let total_progress: u32 =
                self.components.iter().map(|c| c.progress as u32).sum();
            return (total_progress / self.components.len() as u32) as u8;
        }

        let weighted_progress: u64 = self
            .components
            .iter()
            .map(|c| {
                let size = self.component_download_sizes.get(&c.name).unwrap_or(&0);
                (c.progress as u64) * size
            })
            .sum();

        ((weighted_progress * 100) / (total_download_size * 100)) as u8
    }
}

pub fn init_dbus_properties(
    claim: &Claim,
    iface: &InterfaceRef<UpdateAgentManager<UpdateProgress>>,
) {
    let progress = &mut iface.get_mut().0;

    progress.components = claim
        .manifest_components()
        .iter()
        .map(|c| ComponentStatus {
            name: c.name.clone(),
            state: ComponentState::None,
            progress: 0,
        })
        .collect();

    // Store download sizes from sources
    progress.component_download_sizes = claim
        .sources()
        .iter()
        .map(|(name, source)| (name.clone(), source.size))
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

#[cfg(test)]
mod tests {
    use super::*;
    use orb_update_agent_dbus::{ComponentState, ComponentStatus};

    #[test]
    fn test_weighted_progress_calculation() {
        let mut progress = UpdateProgress {
            components: vec![
                ComponentStatus {
                    name: "small".to_string(),
                    state: ComponentState::Downloading,
                    progress: 50, // 50% of 100MB = 50MB worth of progress
                },
                ComponentStatus {
                    name: "large".to_string(),
                    state: ComponentState::Downloading,
                    progress: 25, // 25% of 900MB = 225MB worth of progress
                },
            ],
            overall_status: UpdateAgentState::Downloading,
            component_download_sizes: {
                let mut sizes = HashMap::new();
                sizes.insert("small".to_string(), 100_000_000); // 100MB
                sizes.insert("large".to_string(), 900_000_000); // 900MB
                sizes
            },
        };

        // Total size: 1000MB
        // Weighted progress: (50 * 100MB) + (25 * 900MB) = 5000MB + 22500MB = 27500MB
        // Overall progress: (27500MB * 100) / (1000MB * 100) = 27%
        assert_eq!(progress.overall_progress(), 27);
    }

    #[test]
    fn test_fallback_to_simple_average_when_no_sizes() {
        let mut progress = UpdateProgress {
            components: vec![
                ComponentStatus {
                    name: "comp1".to_string(),
                    state: ComponentState::Downloading,
                    progress: 30,
                },
                ComponentStatus {
                    name: "comp2".to_string(),
                    state: ComponentState::Downloading,
                    progress: 70,
                },
            ],
            overall_status: UpdateAgentState::Downloading,
            component_download_sizes: HashMap::new(), // No size information
        };

        // Should fallback to simple average: (30 + 70) / 2 = 50
        assert_eq!(progress.overall_progress(), 50);
    }

    #[test]
    fn test_empty_components() {
        let progress = UpdateProgress {
            components: vec![],
            overall_status: UpdateAgentState::None,
            component_download_sizes: HashMap::new(),
        };

        assert_eq!(progress.overall_progress(), 0);
    }

    #[test]
    fn test_missing_size_for_component() {
        let mut progress = UpdateProgress {
            components: vec![
                ComponentStatus {
                    name: "known".to_string(),
                    state: ComponentState::Downloading,
                    progress: 50,
                },
                ComponentStatus {
                    name: "unknown".to_string(),
                    state: ComponentState::Downloading,
                    progress: 75,
                },
            ],
            overall_status: UpdateAgentState::Downloading,
            component_download_sizes: {
                let mut sizes = HashMap::new();
                sizes.insert("known".to_string(), 500_000_000); // 500MB
                                                                // "unknown" component size is missing
                sizes
            },
        };

        // Component overall progress: known=10% (50% through downloading), unknown=15% (75% through downloading)  
        // Only the "known" component should contribute to weighted progress (unknown has 0 size)
        // Weighted progress: (10 * 500MB) + (15 * 0MB) = 5000MB + 0MB = 5000MB
        // Overall progress: (5000MB * 100) / (500MB * 100) = 10%
        assert_eq!(progress.overall_progress(), 10);
    }
}
