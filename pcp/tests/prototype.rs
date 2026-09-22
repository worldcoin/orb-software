#[allow(dead_code)]
#[path = "../examples/build_pcp.rs"]
mod example;

#[test]
fn version_device_and_redaction_matrix_round_trips() {
    example::run().expect("synthetic PCP round-trip failed");
}
