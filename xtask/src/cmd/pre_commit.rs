use cmd_lib::run_cmd;

pub fn run() {
    run_cmd! {
        cargo clippy --all --all-features --all-targets --no-deps -- -D warnings;
        cargo fmt;
        taplo format;
    }
    .unwrap();
}
