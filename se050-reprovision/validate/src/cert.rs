use std::str::FromStr;

use eyre::{Context, Result};
use orb_se050::{
    certs::{ChipUniquePubkey, UnixTime},
    extra_data::ChipId,
};

/// Represents a validated NXP chip-unique certificate. Instantiation can be done
/// can be done via [`Self::parse_pem`] or [`Self::from_str`].
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OrbNxpCert {
    pub(crate) chip_unique_pubkey: ChipUniquePubkey,
    pub(crate) chip_id: ChipId,
}

impl OrbNxpCert {
    /// Parses the chip_unique certificate from a PEM file. Uses `current_time`
    /// to check that the certificate has not expired.
    pub fn parse_pem(pem_data: &str, current_time: UnixTime) -> Result<Self> {
        let (chip_unique_pubkey, chip_id) =
            orb_se050::certs::verify_cert(pem_data, current_time)
                .wrap_err("failed to verify nxp certificate")?;

        Ok(Self {
            chip_unique_pubkey,
            chip_id,
        })
    }
}

impl FromStr for OrbNxpCert {
    type Err = eyre::Report;

    fn from_str(pem_data: &str) -> Result<Self, Self::Err> {
        Self::parse_pem(pem_data, UnixTime::now())
    }
}
