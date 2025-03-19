use eyre::Context;
use orb_cellcom::ServingCell;

use crate::CellTower;

use super::{private, CellInfoProvider};

#[derive(Debug)]
pub struct CellcomProvider<'a> {
    pub serving_cell: &'a ServingCell,
}

impl CellInfoProvider for CellcomProvider<'_> {}
impl private::Sealed for CellcomProvider<'_> {
    type Err = eyre::Report;

    fn populate(
        &self,
        request: &mut crate::GeolocationRequest,
    ) -> Result<(), Self::Err> {
        let serving_cell = &self.serving_cell;

        let cell_towers = vec![CellTower {
            cell_id: u32::from_str_radix(&serving_cell.cell_id, 16)
                .wrap_err("failed to parse cell_id as hex")?,
            location_area_code: None,
            mobile_country_code: serving_cell.mcc,
            mobile_network_code: serving_cell.mnc,
            age: None,
            signal_strength: serving_cell.rssi,
            timing_advance: None,
        }];

        request.home_mobile_country_code = serving_cell.mcc;
        request.home_mobile_network_code = serving_cell.mnc;
        request.radio_type = Some(serving_cell.network_type.clone());
        request.cell_towers = cell_towers;

        Ok(())
    }
}
