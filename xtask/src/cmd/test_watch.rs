use crate::cmd::cmd;
use clap::Args as ClapArgs;
use color_eyre::Result;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Arguments to pass to `cargo nextest run`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub packages: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    let mut cmd_args = vec!["bacon", "--headless", "nextest", "--"];
    cmd_args.extend(args.packages.iter().map(String::as_str));
    cmd(&cmd_args)?;

    Ok(())
}
