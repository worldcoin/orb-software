use crate::cmd::cmd;
use cargo_metadata::MetadataCommand;
use clap::Args as ClapArgs;
use color_eyre::Result;

const TARGET: &str = "aarch64-linux-android";

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
    /// Only attempt these crates instead of every workspace member.
    pub pkgs: Vec<String>,
}

/// Attempts to build every requested crate (default: the whole workspace)
/// for Android, reporting which succeed and which don't. Not every crate is
/// expected to build: this is a discovery sweep, not a release gate.
pub fn run(args: Args) -> Result<()> {
    let Args { release, pkgs } = args;

    let pkgs = if pkgs.is_empty() {
        let md = MetadataCommand::new().no_deps().exec()?;
        let mut names: Vec<String> = md
            .workspace_packages()
            .into_iter()
            .map(|pkg| pkg.name.as_str().to_owned())
            .collect();
        names.sort();
        names
    } else {
        pkgs
    };

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for pkg in &pkgs {
        println!("\n=== building `{pkg}` for {TARGET} ===");

        let mut cmd_args =
            vec!["cargo", "build", "--target", TARGET, "-p", pkg.as_str()];
        if release {
            cmd_args.push("--release");
        }

        if cmd(&cmd_args).is_ok() {
            succeeded.push(pkg.clone());
        } else {
            failed.push(pkg.clone());
        }
    }

    println!("\n=== {TARGET} build sweep summary ===");
    println!("succeeded ({}):", succeeded.len());
    for pkg in &succeeded {
        println!("  {pkg}");
    }
    println!("failed ({}):", failed.len());
    for pkg in &failed {
        println!("  {pkg}");
    }

    Ok(())
}
