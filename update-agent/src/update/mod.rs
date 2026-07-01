use orb_dogd::MetricEmitter;
use orb_update_agent_core::{Component, Slot};

pub mod can;
pub mod capsule;
pub mod gpt;
pub mod raw;

pub trait Update {
    fn update<R, M>(&self, slot: Slot, src: R, metrics: &M) -> eyre::Result<()>
    where
        R: std::io::Read + std::io::Seek,
        M: MetricEmitter;
}

impl Update for Component {
    fn update<R, M>(&self, slot: Slot, src: R, metrics: &M) -> eyre::Result<()>
    where
        R: std::io::Read + std::io::Seek,
        M: MetricEmitter,
    {
        match self {
            Component::Can(c) => c.update(slot, src, metrics),
            Component::Gpt(c) => c.update(slot, src, metrics),
            Component::Raw(c) => c.update(slot, src, metrics),
            Component::Capsule(c) => c.update(slot, src, metrics),
        }
    }
}

#[cfg(test)]
mod tests;

pub use can::{try_mcu_set_static_fan_speed, RECOVERY_STATIC_FAN_SPEED_PERCENTAGE};
