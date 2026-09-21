#[allow(dead_code)]
#[path = "../examples/build_pcp.rs"]
mod example;

#[test]
fn all_versions_round_trip_with_and_without_biometrics() {
    example::run().expect("synthetic PCP round-trip failed");
}
