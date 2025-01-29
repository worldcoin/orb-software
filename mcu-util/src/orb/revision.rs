use std::fmt::{Display, Formatter};

use orb_mcu_interface::orb_messages;

#[derive(Clone, Debug, Default)]
pub struct OrbRevision(pub orb_messages::Hardware);

impl Display for OrbRevision {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.0.version
            == i32::from(orb_messages::hardware::OrbVersion::HwVersionUnknown)
        {
            write!(f, "unknown")
        } else if self.0.version
            < orb_messages::hardware::OrbVersion::HwVersionDiamondPoc1 as i32
        {
            write!(f, "EVT{:?}", self.0.version)
        } else {
            write!(
                f,
                "Diamond_B{:?}",
                self.0.version
                    - orb_messages::hardware::OrbVersion::HwVersionDiamondPoc1 as i32
                    + 1
            )
        }
    }
}
