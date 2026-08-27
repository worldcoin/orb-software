use crate::cmd::target::unsupported_crates;
use crate::cmd::{args, cmd, cmd_captured};
use cargo_metadata::{Metadata, MetadataCommand};
use clap::Args as ClapArgs;
use color_eyre::{eyre::eyre, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const TARGET: &str = "aarch64-linux-android";

#[derive(ClapArgs, Debug)]
pub struct BuildArgs {
    /// Build in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Builds the whole workspace for Android, skipping crates marked
/// unsupported via `[package.metadata.orb] unsupported_targets`. Meant for
/// CI: any failure among the non-excluded crates is a hard error.
pub fn run_build(args: BuildArgs) -> Result<()> {
    let md = MetadataCommand::new().no_deps().exec()?;
    run_build_with(&md, args)
}

/// Like [`run_build`], but reuses metadata the caller already fetched
/// instead of shelling out to `cargo metadata` again.
fn run_build_with(md: &Metadata, args: BuildArgs) -> Result<()> {
    let BuildArgs { release } = args;
    let excludes = unsupported_crates(md, TARGET);

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
