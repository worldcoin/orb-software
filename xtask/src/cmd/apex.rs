use crate::cmd::android::{run_build_with, BuildArgs, TARGET};
use crate::cmd::{args, cmd, cmd_captured};
use cargo_metadata::{Metadata, MetadataCommand};
use color_eyre::{eyre::eyre, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::info;

fn apex_name(cargo_pkg_name: &str) -> String {
    format!(
        "com.toolsforhumanity.{}",
        cargo_pkg_name.replace(['-', '_'], ".")
    )
}

/// Stages one APEX payload directory per Android-supported binary crate:
/// `<staging_dir>/<crate>/content/{bin/<binary>,etc/init.rc}` plus sidecar
/// `apex_manifest.json`, `canned_fs_config`, and `file_contexts` - the
/// inputs `nix/packages/android-apex.nix`'s `apexer` needs to produce a
/// signed `.apex`.
fn run_payload(
    md: &Metadata,
    staging_dir: &Path,
    args: BuildArgs,
) -> Result<Vec<String>> {
    // Rebuild first, so the binaries staged below aren't stale.
    let built = run_build_with(md, args.clone())?;

    let profile_dir = if args.release { "release" } else { "debug" };
    // Absolute, so this works regardless of the invoking cwd.
    let target_dir = md.target_directory.as_std_path();

    fs::create_dir_all(staging_dir)?;

    let mut staged = Vec::new();

    for pkg in built.into_iter() {
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
        let pkg_out = staging_dir.join(pkg.name.as_str());
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
        // (`system`) is a placeholder, same as TODO_USER/TODO_SELINUX_DOMAIN
        // below.
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

        let apex_name = apex_name(&pkg.name);

        // apex_manifest's `version` must be a monotonically increasing
        // int64, not this crate's semver, so encode the semver into one
        // (assumes minor/patch stay under 1000, true for every crate in this
        // workspace today). The semver itself is kept as `versionName` for
        // humans.
        let version = &pkg.version;
        let version_code =
            version.major * 1_000_000 + version.minor * 1_000 + version.patch;
        fs::write(
            pkg_out.join("apex_manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "name": apex_name,
                "version": version_code,
                "versionName": version.to_string(),
                // Required for `adb install --force-non-staged` (see
                // run_deploy) to activate immediately instead of being
                // rejected/ignored by apexd.
                "supportsRebootlessUpdate": true,
            }))?,
        )?;

        // init only ever reads a single `etc/init.rc` inside an APEX, never a
        // directory of per-binary scripts (see apexSrc's own docs/README.md,
        // `/apex/my.apex@1/etc/init.rc`) - so every binary's service block
        // is concatenated into that one file.
        let init_rc: String = binaries
            .iter()
            .map(|bin| {
                format!(
                    "# TODO: placeholder, not a working init script. Needs a real \
                     SELinux domain (see external/sepolicy) plus a real decision on \
                     class/user/group/oneshot before this can boot the daemon.\n\
                     service {bin} /apex/{apex_name}/bin/{bin}\n    \
                     class TODO_CLASS\n    user TODO_USER\n    group TODO_GROUP\n    \
                     seclabel u:r:TODO_SELINUX_DOMAIN:s0\n",
                )
            })
            .collect();
        fs::create_dir_all(content_dir.join("etc"))?;
        fs::write(content_dir.join("etc/init.rc"), init_rc)?;
        fs_config.push("/etc 1000 1000 0755".to_string());
        fs_config.push("/etc/init.rc 1000 1000 0644".to_string());

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

/// `android-apex` needs no fields beyond [`BuildArgs`]'s.
pub type ApexArgs = BuildArgs;

/// Prints a captured command's stdout+stderr to this process' matching
/// stream (stdout on success, stderr on failure), so a job's own tool
/// output is still visible even though nothing streamed live.
fn print_captured(output: &std::process::Output) {
    let mut stream: Box<dyn Write> = if output.status.success() {
        Box::new(std::io::stdout())
    } else {
        Box::new(std::io::stderr())
    };
    let _ = stream.write_all(&output.stdout);
    let _ = stream.write_all(&output.stderr);
}

/// Stages Android payloads and packages each into a signed
/// `<out_dir>/<apex-manifest-name>.apex` (see [`apex_name`]), using the
/// `build-apex` flake package (`nix/packages/android-apex.nix`). Requires
/// `nix` on PATH. `out_dir` must already exist. Returns the path to each
/// produced `.apex`.
pub fn run_apex(args: ApexArgs) -> Result<Vec<PathBuf>> {
    // Cloned out up front since `args` itself moves into `run_payload`
    // below (it's threaded through to `run_build_with` unchanged).
    let out_dir = args.out_dir.clone();

    // Fetched once and passed into run_payload (which threads it through to
    // run_build_with) below, instead of shelling out to `cargo metadata`
    // again for data that can't change mid-run.
    let md = MetadataCommand::new().no_deps().exec()?;

    // "build-apex" must match the flake output name in
    // nix/shells/flake-outputs.nix (packages."x86_64-linux"."build-apex") -
    // nothing checks the two stay in sync, so grep both if renaming.
    let flake_ref = format!("{}#build-apex", md.workspace_root);

    // A unique dir per invocation: `run_payload` tears down and recreates
    // each package's subdirectory on every call, so a fixed shared path
    // would race across concurrent `android-apex` invocations.
    let payload_out_dir_handle = tempfile::tempdir()?;
    let payload_out_dir = payload_out_dir_handle.path().to_path_buf();
    let packages = run_payload(&md, &payload_out_dir, args)?;

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
    // build-apex invocation is mostly waiting on apexer/aapt2/avbtool/
    // mkfs.erofs subprocesses rather than burning CPU, so a few dozen
    // threads is affordable. Output is only printed after every thread has
    // joined, so no stdout/stderr lock is needed to keep it unmangled.
    let payload_out_dir = &payload_out_dir;
    let out_dir = &out_dir;
    let build_apex_bin = &build_apex_bin;
    let outcomes: HashMap<String, Result<std::process::Output>> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = packages
                .iter()
                .map(|pkg| {
                    scope.spawn(move || {
                        let payload_dir = payload_out_dir.join(pkg);
                        let apex_out = out_dir.join(format!("{}.apex", apex_name(pkg)));
                        let result =
                            cmd_captured(&[build_apex_bin, &payload_dir, &apex_out]);
                        (pkg.clone(), result)
                    })
                })
                .collect();

            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for (pkg, result) in outcomes {
        match result {
            Ok(output) => {
                print_captured(&output);
                if output.status.success() {
                    succeeded.push(pkg);
                } else {
                    failed.push(pkg);
                }
            }
            Err(e) => {
                eprintln!("{pkg}: {e}");
                failed.push(pkg);
            }
        }
    }
    succeeded.sort();
    failed.sort();

    println!("\n=== apex packaging summary ===");
    println!("succeeded ({}): {}", succeeded.len(), succeeded.join(", "));

    if !failed.is_empty() {
        println!("failed ({}): {}", failed.len(), failed.join(", "));
        return Err(eyre!("failed to package APEX for: {}", failed.join(", ")));
    }

    Ok(succeeded
        .into_iter()
        .map(|pkg| out_dir.join(format!("{}.apex", apex_name(&pkg))))
        .collect())
}
