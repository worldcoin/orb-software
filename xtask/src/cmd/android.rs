use crate::cmd::{args, cmd, cmd_captured};
use cargo_metadata::{Metadata, MetadataCommand};
use clap::Args as ClapArgs;
use color_eyre::{eyre::eyre, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const TARGET: &str = "aarch64-linux-android";

/// Names of workspace packages whose `[package.metadata.orb]
/// unsupported_targets` lists `aarch64-linux-android` (same mechanism
/// `ci/rust_ci_helper.py` uses for its Darwin exclusions).
fn unsupported_packages(md: &Metadata) -> Result<Vec<String>> {
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
/// CI: any failure among the non-excluded crates is a hard error.
pub fn run_build(args: BuildArgs) -> Result<()> {
    let md = MetadataCommand::new().no_deps().exec()?;
    run_build_with(&md, args)
}

/// Like [`run_build`], but reuses metadata the caller already fetched
/// instead of shelling out to `cargo metadata` again.
fn run_build_with(md: &Metadata, args: BuildArgs) -> Result<()> {
    let BuildArgs { release } = args;
    let excludes = unsupported_packages(md)?;

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

/// Stages one APEX payload directory per Android-supported binary crate:
/// `<out_dir>/<crate>/content/{bin/<binary>,etc/init/<binary>.rc}` plus
/// sidecar `apex_manifest.json`, `canned_fs_config`, and `file_contexts` -
/// the inputs `nix/packages/android-apex.nix`'s `apexer` needs to produce a
/// signed `.apex`.
///
/// The manifest name, `etc/init/*.rc` contents, and `file_contexts` are
/// TODO placeholders needing real naming and SELinux details.
fn run_payload(md: &Metadata, out_dir: &Path, release: bool) -> Result<Vec<String>> {
    // Rebuild first, so the binaries staged below aren't stale.
    run_build_with(md, BuildArgs { release })?;

    let excluded = unsupported_packages(md)?;
    let profile_dir = if release { "release" } else { "debug" };
    // Absolute, so this works regardless of the invoking cwd.
    let target_dir = md.target_directory.as_std_path();

    fs::create_dir_all(out_dir)?;

    let mut staged = Vec::new();

    for pkg in md.workspace_packages() {
        if excluded.iter().any(|e| e == pkg.name.as_str()) {
            continue;
        }
        let binaries: Vec<&str> = pkg
            .targets
            .iter()
            .filter(|t| t.is_bin())
            .map(|t| t.name.as_str())
            .collect();
        if binaries.is_empty() {
            continue;
        }

        // content/ must hold exactly the APEX's root filesystem; the
        // sidecar manifest/canned_fs_config/file_contexts live outside it,
        // or apexer's e2fsdroid mistakes them for payload files.
        let pkg_out = out_dir.join(pkg.name.as_str());
        // Recreate from scratch, so a stale binary/init script from a prior
        // run isn't bundled into an APEX whose canned_fs_config no longer
        // lists it.
        if pkg_out.exists() {
            fs::remove_dir_all(&pkg_out)?;
        }
        let content_dir = pkg_out.join("content");
        let bin_out = content_dir.join("bin");
        fs::create_dir_all(&bin_out)?;

        // We author every path ourselves, so canned_fs_config is emitted
        // directly instead of walking the tree afterward. uid/gid 1000
        // (`system`) is a placeholder, same status as TODO_USER/
        // TODO_SELINUX_DOMAIN below.
        let mut fs_config = vec![
            "/ 1000 1000 0755".to_string(),
            "/apex_manifest.pb 1000 1000 0644".to_string(),
            "/bin 1000 1000 0755".to_string(),
        ];

        for bin in &binaries {
            let src = target_dir.join(TARGET).join(profile_dir).join(bin);
            let dst = bin_out.join(bin);
            fs::copy(&src, &dst).map_err(|e| {
                eyre!("failed to copy {} -> {}: {e}", src.display(), dst.display())
            })?;
            fs_config.push(format!("/bin/{bin} 1000 1000 0755"));
        }

        // TODO: "com.worldcoin.orb.*" is a placeholder - confirm the real
        // naming scheme before shipping. `-` isn't valid in Android package
        // names (Java identifiers joined by dots); aapt2 rejects it, so
        // it's swapped for `_`.
        let pkg_name = pkg
            .name
            .as_str()
            .strip_prefix("orb-")
            .unwrap_or(pkg.name.as_str());
        let apex_name = format!("com.worldcoin.orb.{}", pkg_name.replace('-', "_"));

        // apex_manifest's `version` must be a monotonically increasing
        // int64, not the semver this crate is actually released under (see
        // Cargo.toml), so encode that semver into one instead of a
        // placeholder constant - assumes minor/patch stay under 1000,
        // true for every crate version in this workspace today. The
        // original string is kept as `versionName` for humans.
        let version = &pkg.version;
        let version_code =
            version.major * 1_000_000 + version.minor * 1_000 + version.patch;
        fs::write(
            pkg_out.join("apex_manifest.json"),
            format!(
                "{{\n  \"name\": \"{apex_name}\",\n  \"version\": {version_code},\n  \"versionName\": \"{version}\"\n}}\n"
            ),
        )?;

        let init_dir = content_dir.join("etc/init");
        fs::create_dir_all(&init_dir)?;
        fs_config.push("/etc 1000 1000 0755".to_string());
        fs_config.push("/etc/init 1000 1000 0755".to_string());
        for bin in &binaries {
            fs::write(
                init_dir.join(format!("{bin}.rc")),
                format!(
                    "# TODO: placeholder, not a working init script. Needs a real \
                     SELinux domain (see external/sepolicy) plus a real decision on \
                     class/user/group/oneshot before this can boot the daemon.\n\
                     service {bin} /apex/{apex_name}/bin/{bin}\n    \
                     class TODO_CLASS\n    user TODO_USER\n    group TODO_GROUP\n    \
                     seclabel u:r:TODO_SELINUX_DOMAIN:s0\n",
                    bin = bin,
                ),
            )?;
            fs_config.push(format!("/etc/init/{bin}.rc 1000 1000 0644"));
        }

        fs_config.sort();
        fs::write(
            pkg_out.join("canned_fs_config"),
            fs_config.join("\n") + "\n",
        )?;

        // TODO: placeholder SELinux context, matches no real sepolicy type
        // yet. A single catch-all regex entry is valid file_contexts
        // syntax, unlike the per-path listing in canned_fs_config.
        fs::write(
            pkg_out.join("file_contexts"),
            "(/.*)?    u:object_r:TODO_SELINUX_CONTEXT:s0\n",
        )?;

        staged.push(pkg.name.as_str().to_owned());
    }

    Ok(staged)
}

#[derive(ClapArgs, Debug)]
pub struct ApexArgs {
    /// Directory to write the resulting `<crate>.apex` files into.
    #[arg(long, default_value = "target/android-apex")]
    pub out_dir: PathBuf,
    /// Stage/build binaries in release mode.
    #[arg(long)]
    pub release: bool,
}

/// Stages Android payloads and packages each into a signed
/// `<out_dir>/<crate>.apex`, using the `build-apex` tool from
/// `nix/packages/android-apex.nix` (exposed as the `build-apex` flake
/// package). Requires `nix` on PATH.
pub fn run_apex(args: ApexArgs) -> Result<()> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(eyre!(
            "android-apex packaging requires an x86_64-linux host: the \
             `build-apex` flake package (see nix/packages/android-apex.nix) \
             only ships a Linux/x86_64 prebuilt mkfs.erofs and is only \
             exposed for that system in nix/shells/flake-outputs.nix"
        ));
    }

    let ApexArgs { out_dir, release } = args;

    // Fetched once and threaded through run_build_with/run_payload/
    // unsupported_packages below, instead of each shelling out to `cargo
    // metadata` again for data that can't change mid-run.
    let md = MetadataCommand::new().no_deps().exec()?;

    // Absolute, so this works regardless of the invoking cwd. "build-apex"
    // must match the flake output name in nix/shells/flake-outputs.nix
    // (packages."x86_64-linux"."build-apex") - nothing checks the two
    // stay in sync, so grep for "build-apex" in both places if renaming.
    let flake_ref = format!("{}#build-apex", md.workspace_root);

    // A unique dir per invocation: `run_payload` tears down and recreates
    // each package's subdirectory on every call, so a fixed shared path
    // would race across concurrent `android-apex` invocations.
    let payload_out_dir_handle = tempfile::tempdir()?;
    let payload_out_dir = payload_out_dir_handle.path().to_path_buf();
    let packages = run_payload(&md, &payload_out_dir, release)?;

    // Recreate from scratch, so a stale `.apex` from a crate that's now
    // unsupported/removed doesn't linger for CI to pick up.
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)?;
    }
    fs::create_dir_all(&out_dir)?;

    // A unique dir per invocation: `nix build --out-link` replaces
    // whatever's at that path, so a fixed path would race across runs.
    let build_apex_link_dir = tempfile::tempdir()?;
    let build_apex_link = build_apex_link_dir.path().join("build-apex");
    cmd(&args![
        "nix",
        "build",
        "--extra-experimental-features",
        "nix-command flakes",
        &flake_ref,
        "--out-link",
        &build_apex_link,
    ])?;
    let build_apex_bin = build_apex_link.join("bin/build-apex");

    // Keep going on failure, so one bad crate doesn't block every other
    // APEX that's otherwise ready. One thread per package, uncapped: each
    // build-apex invocation spends most of its time waiting on apexer/
    // aapt2/avbtool/mkfs.erofs subprocesses rather than burning CPU itself,
    // so it's not purely core-bound - a few dozen threads is affordable.
    let payload_out_dir = &payload_out_dir;
    let out_dir = &out_dir;
    let build_apex_bin = &build_apex_bin;
    let outcomes = std::thread::scope(|scope| {
        let handles: Vec<_> = packages
            .iter()
            .map(|pkg| {
                scope.spawn(move || -> Result<String, String> {
                    let payload_dir = payload_out_dir.join(pkg);
                    let apex_out = out_dir.join(format!("{pkg}.apex"));
                    let result =
                        cmd_captured(&args![build_apex_bin, &payload_dir, &apex_out]);

                    match result {
                        Ok(output) => {
                            let mut out = std::io::stdout().lock();
                            let _ = out.write_all(&output);
                            let _ = writeln!(
                                out,
                                "packaged `{pkg}` -> {}",
                                apex_out.display()
                            );
                            Ok(pkg.clone())
                        }
                        Err(e) => {
                            let mut err = std::io::stderr().lock();
                            let _ = writeln!(err, "failed to package `{pkg}`: {e}");
                            Err(pkg.clone())
                        }
                    }
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });

    let mut succeeded: Vec<String> =
        outcomes.iter().cloned().filter_map(Result::ok).collect();
    let mut failed: Vec<String> =
        outcomes.into_iter().filter_map(Result::err).collect();
    succeeded.sort();
    failed.sort();

    println!("\n=== apex packaging summary ===");
    println!("succeeded ({}): {}", succeeded.len(), succeeded.join(", "));

    if !failed.is_empty() {
        println!("failed ({}): {}", failed.len(), failed.join(", "));
        return Err(eyre!("failed to package APEX for: {}", failed.join(", ")));
    }

    Ok(())
}
