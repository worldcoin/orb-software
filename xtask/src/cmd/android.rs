use crate::cmd::cmd;
use cargo_metadata::MetadataCommand;
use clap::Args as ClapArgs;
use color_eyre::Result;

const TARGET: &str = "aarch64-linux-android";

/// Names of workspace packages whose `[package.metadata.orb]
/// unsupported_targets` lists `aarch64-linux-android` - the same mechanism
/// `ci/rust_ci_helper.py` already uses to exclude Darwin-incompatible
/// crates from `cargo nextest run --workspace`.
fn unsupported_packages() -> Result<Vec<String>> {
    let md = MetadataCommand::new().no_deps().exec()?;
    let mut names: Vec<String> = md
        .workspace_packages()
        .into_iter()
        .filter(|pkg| {
            pkg.metadata
                .get("orb")
                .and_then(|orb| orb.get("unsupported_targets"))
                .and_then(|targets| targets.as_array())
                .is_some_and(|targets| {
                    targets.iter().any(|t| t.as_str() == Some(TARGET))
                })
        })
        .map(|pkg| pkg.name.as_str().to_owned())
        .collect();
    names.sort();

    Ok(names)
}

#[derive(ClapArgs, Debug)]
pub struct BuildArgs {
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Builds the whole workspace for Android, skipping crates marked
/// unsupported via `[package.metadata.orb] unsupported_targets`. Meant for
/// CI: unlike `android-sweep`, a build failure here is an error.
pub fn run_build(args: BuildArgs) -> Result<()> {
    let BuildArgs { release } = args;
    let excludes = unsupported_packages()?;

    println!(
        "skipping (unsupported on {TARGET}): {}",
        excludes.join(", ")
    );

    let mut cmd_args = vec!["cargo", "build", "--workspace", "--target", TARGET];
    if release {
        cmd_args.push("--release");
    }
    for pkg in &excludes {
        cmd_args.push("--exclude");
        cmd_args.push(pkg);
    }

    cmd(&cmd_args)
}

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
