use cmd_lib::run_cmd;
use color_eyre::Result;

pub fn run() -> Result<()> {
    run_cmd! {
        cargo clippy --all --all-features --all-targets --no-deps -- -D warnings;
        cargo fmt;
        taplo format;
    }?;

    Ok(())
}
