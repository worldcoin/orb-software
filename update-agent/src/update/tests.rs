use std::fs::File;

use crate::update::Update;

/// test updating the main mcu
#[test]
//#[cfg(feature = "can-update-test")]
#[ignore = "needs vcan interface"]
pub fn try_can_update() -> eyre::Result<()> {
    orb_telemetry::TelemetryConfig::new().try_init().ok();

    let mut file = File::open("/mnt/scratch/app_mcu_main_test.bin")?;

    let can = orb_update_agent_core::components::Can {
        address: 0x1, // main mcu
        bus: "can0".to_string(),
        redundancy: orb_update_agent_core::components::Redundancy::Single,
    };
    can.update(orb_update_agent_core::Slot::A, &mut file)?;

    Ok(())
}
