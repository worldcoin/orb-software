use cmd_lib::run_cmd;
use color_eyre::Result;

#[derive(clap::Args, Debug)]
pub struct Args {}

impl Args {
    pub fn run(self) -> Result<()> {
        run_cmd! {
            cargo clippy --all --all-features --all-targets --no-deps -- -D warnings;
            cargo fmt;
            taplo format;
        }?;

        Ok(())
    }
}
