use cargo_metadata::{Metadata, MetadataCommand};
use clap::Args as ClapArgs;
use color_eyre::{eyre::eyre, Result};
use std::collections::BTreeSet;

/// Names of workspace packages whose `[package.metadata.orb]
/// unsupported_targets` lists `target` (same mechanism
/// `ci/rust_ci_helper.py` uses for its Darwin exclusions). A `BTreeSet` so
/// callers get both a fast `.contains()` and a deterministic, sorted
/// iteration order for printing.
pub fn unsupported_packages(md: &Metadata, target: &str) -> BTreeSet<String> {
    md.workspace_packages()
        .into_iter()
        .filter(|pkg| {
            pkg.metadata
                .get("orb")
                .and_then(|orb| orb.get("unsupported_targets"))
                .and_then(|targets| targets.as_array())
                .is_some_and(|targets| {
                    targets.iter().any(|t| t.as_str() == Some(target))
                })
        })
        .map(|pkg| pkg.name.as_str().to_owned())
        .collect()
}

#[derive(ClapArgs, Debug)]
pub struct SupportedArgs {
    /// Crate to check.
    pub pkg: String,
    /// Target triple to check support for, e.g. `aarch64-linux-android`.
    #[arg(long)]
    pub target: String,
}

/// Whether `pkg` is excluded for `target` via `[package.metadata.orb]
/// unsupported_targets`. Pure metadata inspection: doesn't build or package
/// anything, so it's cheap enough for callers to use as an up-front branch
/// instead of parsing a build's error output.
///
/// `Ok(true)`: is a workspace member and isn't excluded for `target`.
/// `Ok(false)`: is a workspace member, but is explicitly excluded for
/// `target` via `unsupported_targets`.
/// `Err(..)`: no workspace member by that name (or `cargo metadata` failed).
fn check_support(md: &Metadata, pkg: &str, target: &str) -> Result<bool> {
    let is_member = md
        .workspace_packages()
        .into_iter()
        .any(|p| p.name.as_str() == pkg);
    if !is_member {
        return Err(eyre!("`{pkg}` is not a workspace member"));
    }

    Ok(!unsupported_packages(md, target).contains(pkg))
}

/// Runs [`check_support`] against live `cargo metadata`.
pub fn run_supported(args: SupportedArgs) -> Result<bool> {
    let SupportedArgs { pkg, target } = args;
    let md = MetadataCommand::new().no_deps().exec()?;

    check_support(&md, &pkg, &target)
}
