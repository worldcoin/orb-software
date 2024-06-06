use cmd_lib::run_cmd;

pub fn is_recovery_mode_detected() -> bool {
    run_cmd! {
        info "Running lsusb";
        lsusb | rg "NVIDIA Corp. APX";
    }
    .is_ok()
}
