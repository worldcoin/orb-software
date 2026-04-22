pub mod attributes;
pub mod extra_data;

#[cfg(test)]
mod example_data {
    pub const ORB_SESSION_KEY: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/example_data/60000000.extra.raw"
    ));

    pub const ORB_ATTESTATION_KEY: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/example_data/60000001.extra.raw"
    ));

    pub const ORB_IRIS_KEY: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/example_data/60000002.extra.raw"
    ));
}
