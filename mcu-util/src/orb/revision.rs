use orb_mcu_interface::orb_messages::mcu_main as main_messaging;
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Default)]
pub struct OrbRevision(pub main_messaging::Hardware);

impl Display for OrbRevision {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.0.version
            == i32::from(main_messaging::hardware::OrbVersion::HwVersionUnknown)
        {
            write!(f, "unknown")
        } else if self.0.version
            < main_messaging::hardware::OrbVersion::HwVersionDiamondPoc1 as i32
        {
            write!(f, "EVT{:?}", self.0.version)
        } else {
            write!(
                f,
                "Diamond_B{:?}",
                self.0.version
                    - main_messaging::hardware::OrbVersion::HwVersionDiamondPoc1 as i32
                    + 1
            )
        }
    }
}
