use crate::cmd::cmd;
use crate::cmd::target::unsupported_packages;
use cargo_metadata::{Metadata, MetadataCommand, Package};
use clap::Args as ClapArgs;
use color_eyre::Result;
use std::path::PathBuf;

pub(crate) const TARGET: &str = "aarch64-linux-android";
pub(crate) const DEFAULT_APEX_OUT_DIR: &str = "target/android-apex";

/// Shared by `android-build`/`android-apex`/`android-deploy`
#[derive(ClapArgs, Debug, Clone)]
pub struct BuildArgs {
    /// Crate to build/package/install. If omitted, applies to every
    /// Android-supported crate in the workspace.
    #[arg(value_parser = parse_package)]
    pub pkg: Option<Package>,
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
    /// Directory to write the resulting `<apex-manifest-name>.apex` files
    /// into. Only used when packaging/installing an APEX.
    #[arg(long, default_value = DEFAULT_APEX_OUT_DIR)]
    pub out_dir: PathBuf,
}

/// Resolves and validates a `pkg` CLI argument at parse time: it must name a
/// real workspace package that's actually supported on `TARGET`, so
/// `build_with` below never has to re-check either condition.
fn parse_package(name: &str) -> std::result::Result<Package, String> {
    let md = MetadataCommand::new()
        .no_deps()
        .exec()
        .map_err(|e| format!("failed to run `cargo metadata`: {e}"))?;

    let pkg = md
        .workspace_packages()
        .into_iter()
        .find(|p| p.name.as_str() == name)
        .cloned()
        .ok_or_else(|| format!("no such package `{name}` in the workspace"))?;

    if unsupported_packages(&md, TARGET).contains(pkg.name.as_str()) {
        return Err(format!("`{name}` is unsupported on {TARGET}"));
    }

    Ok(pkg)
}

/// Builds the whole workspace for Android, skipping crates marked
/// unsupported via `[package.metadata.orb] unsupported_targets`. Meant for
/// CI: any failure among the non-excluded crates is a hard error.
pub fn run_build(args: BuildArgs) -> Result<()> {
    let md = MetadataCommand::new().no_deps().exec()?;
    run_build_with(&md, args).map(|_| ())
}

/// Like [`run_build`], but reuses metadata the caller already fetched
/// instead of shelling out to `cargo metadata` again - shared with
/// [`crate::cmd::apex::run_apex`], which needs a fresh build of the same
/// target before staging APEX payloads.
pub(crate) fn run_build_with(md: &Metadata, args: BuildArgs) -> Result<Vec<Package>> {
    build_with(md, args.pkg, args.release, &["build"])
}

/// CLI args for `android-test` - same as [`BuildArgs`] minus `out_dir`,
/// which only matters when packaging an APEX.
#[derive(ClapArgs, Debug, Clone)]
pub struct TestArgs {
    /// Crate to build. If omitted, applies to every Android-supported crate
    /// in the workspace.
    #[arg(value_parser = parse_package)]
    pub pkg: Option<Package>,
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Compiles (but doesn't run - there's no Android device/emulator here)
/// each crate's test binaries for Android via `cargo build --tests`. Catches
/// API mismatches between target-specific modules (e.g.
/// `orb_id_linux.rs`/`orb_id_android.rs`) that only show up in test code,
/// which `android-build` alone can't see.
pub fn run_build_test(args: TestArgs) -> Result<()> {
    let md = MetadataCommand::new().no_deps().exec()?;
    build_with(&md, args.pkg, args.release, &["build", "--tests"]).map(|_| ())
}

fn build_with(
    md: &Metadata,
    pkg: Option<Package>,
    release: bool,
    subcmd: &[&str],
) -> Result<Vec<Package>> {
    let excludes = unsupported_packages(md, TARGET);

    let mut cmd_args = vec!["cargo"];
    cmd_args.extend_from_slice(subcmd);
    cmd_args.push("--target");
    cmd_args.push(TARGET);
    if release {
        cmd_args.push("--release");
    }

    let built: Vec<Package> = match &pkg {
        Some(pkg) => {
            cmd_args.push("-p");
            cmd_args.push(pkg.name.as_str());
            vec![pkg.clone()]
        }
        None => {
            cmd_args.push("--workspace");
            for pkg in &excludes {
                cmd_args.push("--exclude");
                cmd_args.push(pkg);
            }
            md.workspace_packages()
                .into_iter()
                .filter(|p| !excludes.contains(p.name.as_str()))
                .cloned()
                .collect()
        }
    };

    cmd(&cmd_args)?;

    Ok(built)
}
