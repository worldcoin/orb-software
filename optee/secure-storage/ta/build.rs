use optee_utee_build::{Error, RustEdition, TaConfig};

fn main() -> Result<(), Error> {
    let config = TaConfig::new_default_with_cargo_env(
        orb_secure_storage_proto::StorageDomain::WifiProfiles.as_uuid(),
    )?;
    optee_utee_build::build(RustEdition::Edition2024, config)
}
