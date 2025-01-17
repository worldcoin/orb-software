//! `UpdateAgent` is responsible for updating all components that make up the Orb. This includes
//! everything running on the Jetson and both MCUs.
//!
//! Effectively it's a very simple state machine that performs the following steps:
//!
//! 1. read the `versions.json` file on the orb;
//! 2. read the `components.json` file on the orb;
//! 3. check that `versions.json` and `components.json` are consistent with each other;
//! 4. get the update claim, which contains the list of components to be updated, and where to find
//!    them (locally or remotely on a server);
//! 5. validate the update by checking the component versions that it is updating from (that is,
//!    ensure that the versions of the components that are currently running/making up the orb match
//!    those listed in the claim);
//! 6. collect the actual update components, either by checking them on disk or downloading them
//!    from the listed URL;
//! 7. validate the downloaded components by comparing their hashes against those listed in the
//!    manifest;
//! 8. actually perform the update by copying the component to its respective position on the
//!    currently inactive slot.
use std::{
    borrow::Cow,
    collections::HashSet,
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    time::Duration,
};

use crate::update::capsule::{EFI_OS_INDICATIONS, EFI_OS_REQUEST_CAPSULE_UPDATE};
use clap::Parser as _;
use eyre::{bail, ensure, WrapErr};
use nix::sys::statvfs;
use orb_update_agent::{
    component, component::Component, dbus, update, update_component_version_on_disk,
    Args, Settings, BUILD_INFO,
};
use orb_update_agent_core::{
    version_map::SlotVersion, Claim, Slot, VersionMap, Versions,
};
use orb_zbus_proxies::login1;
use slot_ctrl::EfiVar;
use tracing::{debug, error, info, warn};

mod update_agent_result;
use update_agent_result::UpdateAgentResult;

const CFG_DEFAULT_PATH: &str = "/etc/orb_update_agent.conf";
const ENV_VAR_PREFIX: &str = "ORB_UPDATE_AGENT_";
const CFG_ENV_VAR: &str = const_format::concatcp!(ENV_VAR_PREFIX, "CONFIG");
const SYSLOG_IDENTIFIER: &str = "worldcoin-update-agent";

fn main() -> UpdateAgentResult {
    let otel_config = orb_telemetry::OpenTelemetryConfig::new(
        "http://localhost:4317",
        SYSLOG_IDENTIFIER,
        BUILD_INFO.version,
        env::var("ORB_BACKEND")
            .expect("ORB_BACKEND environment variable must be set")
            .to_lowercase(),
    );

    let _telemetry_guard = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .with_opentelemetry(otel_config)
        .init();

    let args = Args::parse();

    match run(&args) {
        Ok(_) => UpdateAgentResult::Success,
        Err(err) => {
            error!("{err:?}");
            err.into()
        }
    }
}

fn get_config_source(args: &Args) -> Cow<'_, Path> {
    if let Some(config) = &args.config {
        info!("using config provided by command line argument: `{config}`");
        Cow::Borrowed(config.as_ref())
    } else if let Some(config) = figment::providers::Env::var(CFG_ENV_VAR) {
        info!("using config set in environment variable `{CFG_ENV_VAR}={config}`");
        Cow::Owned(std::path::PathBuf::from(config))
    } else {
        info!("using default config at `{CFG_DEFAULT_PATH}`");
        Cow::Borrowed(CFG_DEFAULT_PATH.as_ref())
    }
}

fn run(args: &Args) -> eyre::Result<()> {
    // TODO: In the event of a corrupt EFIVAR slot, we would be put into an unrecoverable state
    let active_slot =
        slot_ctrl::get_current_slot().wrap_err("failed getting current slot")?;

    let config_path = get_config_source(args);

    // TODO: Inject active_slot in a more ergonomic way
    let settings = Settings::get(args, config_path, ENV_VAR_PREFIX, active_slot.into())
        .wrap_err("failed reading settings")?;

    let settings_ser = match serde_json::to_string(&settings) {
        Ok(ser) => ser,
        Err(e) => {
            warn!("failed serializing settings as json, printing debug string: {e:?}");
            format!("{settings:?}")
        }
    };
    debug!("running with the following settings: {settings_ser}");

    prepare_environment(&settings).wrap_err("failed preparing environment to run")?;

    let supervisor_proxy = if settings.nodbus || settings.recovery {
        debug!("nodbus flag set or in recovery; not connecting to dbus");
        None
    } else {
        match zbus::blocking::Connection::session()
            .wrap_err("failed establishing a `session` dbus connection")
            .and_then(|conn| {
                dbus::SupervisorProxyBlocking::builder(&conn)
                    .cache_properties(zbus::CacheProperties::No)
                    .build()
                    .wrap_err("failed creating a supervisor dbus proxy")
            }) {
            Ok(proxy) => Some(proxy),
            Err(e) => {
                warn!(
                    "failed connecting to DBus; updates will be downloaded but not installed: \
                     {e:?}"
                );
                None
            }
        }
    };

    info!(
        "reading versions from disk at `{}",
        settings.versions.display()
    );
    let versions_legacy =
        read_versions_on_disk(&settings.versions).wrap_err_with(|| {
            format!(
                "failed reading versions on disk at {}",
                settings.versions.display(),
            )
        })?;

    let mut version_map_dst = settings.versions.clone();
    version_map_dst.set_extension("map");

    debug!(
        "attempting to read the new versions map from file system at `{}`",
        version_map_dst.display(),
    );

    fn try_read_version_map<P: AsRef<Path>>(
        version_path: P,
    ) -> eyre::Result<VersionMap> {
        let contents =
            fs::read(version_path).wrap_err("failed to read file to buffer")?;
        serde_json::from_slice(&contents)
            .wrap_err("failed deserializing file buffer to json")
    }

    let version_map_from_legacy = VersionMap::from_legacy(&versions_legacy);
    let mut version_map = try_read_version_map(&version_map_dst)
        .wrap_err_with(|| {
            format!(
                "failed reading version map from `{}`",
                version_map_dst.display(),
            )
        })
        .map(|version_map| {
            if version_map != version_map_from_legacy {
                warn!(
                    "version map on disk does not match version map constructed from legacy \
                     versions.json; preferring legacy. this will be an error in the future"
                );
                version_map_from_legacy.clone()
            } else {
                version_map
            }
        })
        .unwrap_or_else(|e| {
            info!("unable to read version map from disk; transforming legacy versions: {e:?}");
            version_map_from_legacy
        });

    match serde_json::to_string(&version_map) {
        Ok(s) => info!("versions read from disk: {s}"),
        Err(e) => {
            warn!("failed serializing versions read from disk: {e:?}");
            info!("versions read from disk: {version_map:?}");
        }
    }

    let claim = orb_update_agent::claim::get(&settings, &version_map)
        .wrap_err("unable to get update claim")?;

    match serde_json::to_string(&claim) {
        Ok(s) => info!("update claim received: {s}"),
        Err(e) => {
            warn!("failed serializing update claim as json: {e:?}");
            info!("update claim received: {claim:?}");
        }
    }

    if settings.skip_version_asserts {
        info!("skipping versions asserts requested; skipping update claim validation");
    } else {
        info!("validating update claim against versions on disk");
        validate_claim(&claim, &version_map, settings.active_slot)
            .wrap_err("failed validating update claim against on-disk versions")?;
    }

    info!("cleanup old updates");
    cleanup_old_updates(&settings.downloads, &claim)
        .wrap_err("failed to cleaning up old updates")?;
    info!("check if free space is enough for new update");
    check_for_available_space(&settings.downloads, &claim)
        .wrap_err("failed to check for free space")?;

    info!("fetching and validating components listed in manifest");
    let update_components = fetch_update_components(
        &claim,
        &settings.workspace,
        &settings.downloads,
        supervisor_proxy.as_ref(),
        settings.download_delay,
    )
    .wrap_err("failed fetching update components")?;

    ensure!(!settings.noupdate, "noupdate was requested; bailing");

    let target_slot = settings.active_slot.opposite();
    debug!("!! proceeding with update!!");
    debug!("active slot: {}", settings.active_slot);
    debug!("target slot: {}", target_slot);

    if settings.nodbus || settings.recovery {
        debug!(
            "nodbus option set or in recovery mode; not requesting update permission and \
             performing update immediately"
        );
    } else if let Some(supervisor_proxy) = supervisor_proxy.as_ref() {
        supervisor_proxy.request_update_permission().wrap_err(
            "failed querying supervisor service for update permission; bailing",
        )?;
    } else {
        bail!("no connection to dbus supervisor, bailing");
    }

    // before starting to update components, set the rootfs status for the target slot accordingly
    slot_ctrl::set_rootfs_status(
        slot_ctrl::RootFsStatus::UpdateInProcess,
        target_slot.into(),
    )
    .wrap_err_with(|| {
        format!("failed to set the rootfs status for the target slot {target_slot}")
    })?;

    for component in &update_components {
        info!("running update for component `{}`", component.name());
        component
            .run_update(target_slot, &claim, settings.recovery)
            .wrap_err_with(|| {
                format!(
                    "failed executing update for component `{}`",
                    component.name()
                )
            })?;

        update_component_version_on_disk(
            target_slot,
            component,
            &mut version_map,
            &version_map_dst,
        )
        .wrap_err_with(|| {
            format!(
                "failed updating version for component `{}` at `{}`",
                component.name(),
                version_map_dst.display(),
            )
        })?;
    }

    if claim.manifest().is_normal_update() && !settings.recovery {
        update::gpt::copy_not_updated_redundant_components(
            &claim,
            &update_components,
            settings.active_slot,
            &mut version_map,
            &version_map_dst,
        )
        .wrap_err("failed to copy redundant GPT partitions not listed in manifest")?;
    }

    info!("Executing post update logic");
    finalize(&settings, &claim, version_map, version_map_dst)
        .wrap_err("failed to finalize update")
}

fn read_versions_on_disk<T: AsRef<Path>>(versions_path: T) -> eyre::Result<Versions> {
    let versions_file =
        File::open(versions_path).wrap_err("failed to open versions file")?;
    orb_update_agent::json::deserialize(&versions_file)
        .wrap_err("failed to read versions from file")
}

/// Checks that the versions asserted in the update claim match those recorded on disk.
pub fn validate_claim(
    claim: &Claim,
    version_map: &VersionMap,
    active_slot: Slot,
) -> eyre::Result<()> {
    for component in claim.manifest_components() {
        let name = component.name();
        let Some(slot_version) = version_map.slot_version(component.name()) else {
            info!("component `{name}` in update manifest is not present in versions on device");
            continue;
        };
        match slot_version {
            SlotVersion::Single {
                version: on_disk_version,
            } => {
                if &component.version_assert == on_disk_version {
                    debug!(
                        "single component `{name}`: on disk version matches expected version in \
                         claim"
                    );
                } else if &component.version_upgrade == on_disk_version {
                    debug!(
                        "single component `{name}`: on disk version matches target version in \
                         claim; was it previously updated?"
                    );
                } else {
                    bail!(
                        "failed to validate version of single component `{name}`; on disk \
                         version: {on_disk_version}, expected version: {}, target version: {}",
                        component.version_assert,
                        component.version_upgrade,
                    );
                }
            }
            SlotVersion::Redundant {
                version_a,
                version_b,
            } => {
                let on_disk_version = match active_slot {
                    Slot::A => version_a,
                    Slot::B => version_b,
                };
                ensure!(
                    Some(&component.version_assert) == on_disk_version.as_ref(),
                    "failed validating redundant component `{name}`; manifest expected version: \
                     {expected_version:?}; actual version on disk: {actual_version:?}",
                    expected_version = component.version_assert,
                    actual_version = on_disk_version,
                );
            }
        }
    }
    Ok(())
}

fn fetch_update_components(
    claim: &Claim,
    manifest_dst: &Path,
    dst: &Path,
    supervisor_proxy: Option<&dbus::SupervisorProxyBlocking<'static>>,
    download_delay: Duration,
) -> eyre::Result<Vec<Component>> {
    orb_update_agent::manifest::compare_to_disk(claim.manifest(), manifest_dst)?;
    let mut components = Vec::with_capacity(claim.num_components());
    for (component, source) in claim.iter_components_with_location() {
        let component = component::fetch(
            component,
            &claim.system_components()[component.name()],
            source,
            dst,
            supervisor_proxy,
            download_delay,
        )
        .wrap_err_with(|| {
            format!("failed fetching source for component `{}`", source.name)
        })?;
        components.push(component);
    }
    components
        .iter_mut()
        .try_for_each(|comp| {
            comp.process(dst).wrap_err_with(|| {
                format!(
                    "failed to process update file for component `{}`",
                    comp.name(),
                )
            })
        })
        .wrap_err("failed post processing downloaded components")?;
    Ok(components)
}

fn cleanup_old_updates(dst: &Path, claim: &Claim) -> eyre::Result<()> {
    ensure!(
        dst.is_dir(),
        format!(
            "provided destination `{}` is not a directory",
            dst.display()
        )
    );

    let disk_entries: HashSet<_> = fs::read_dir(dst)?
        .filter_map(|res| res.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|e| !e.is_empty())
        .collect();
    info!("current disk downloaded entries: `{:?}`", disk_entries);

    let claim_entries: HashSet<_> = claim
        .sources()
        .iter()
        .flat_map(|(_, s)| {
            vec![
                s.unique_name(),
                // TODO(andronat): I would like to have a "safer" way to create this names. What if
                // we change these conventions later one?
                format!("{}.{}", s.unique_name(), "verified"),
                format!("{}.{}", s.unique_name(), "uncompressed"),
                format!("{}.{}", s.unique_name(), "uncompressed.verified"),
            ]
        })
        .collect();
    info!("claim entries that won't be deleted: `{:?}`", claim_entries);

    let entries_to_delete = disk_entries.difference(&claim_entries);
    info!("deleting from entries from disk: `{:?}`", entries_to_delete);

    for e in entries_to_delete {
        let e = dst.join(e);
        if e.is_file() {
            fs::remove_file(e)?;
        } else {
            fs::remove_dir_all(e)?;
        }
    }

    Ok(())
}

fn check_for_available_space<P: AsRef<Path>>(
    dst: &P,
    claim: &Claim,
) -> eyre::Result<()> {
    let stats = match statvfs::statvfs(dst.as_ref()) {
        Ok(stats) => stats,
        Err(e) => {
            warn!(
                "failed to get statvfs at `{}`: {e:?}. Assuming: enough space and continue",
                dst.as_ref().display()
            );
            return Ok(());
        }
    };
    let piece_size = if stats.fragment_size() == 0 {
        stats.block_size()
    } else {
        stats.fragment_size()
    };
    if piece_size == 0 {
        warn!(
            "fragment size and block size are both 0 at `{}`. Assuming: enough space and continue",
            dst.as_ref().display()
        );
        return Ok(());
    }
    let available_space = stats.blocks_available() * piece_size;

    // TODO(oldgalileo): Clean up duplicated code, make this better
    // This checks the claim entries against all files in the destination
    // then assumes the intersection of the two sets are the "existing"
    // files. It then sums the filesize of all the existing values to get
    // the size of the currently downloaded components on disk.
    let disk_entries: HashSet<_> = fs::read_dir(dst)?
        .filter_map(|res| res.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|e| !e.is_empty())
        .collect();
    let claim_entries: HashSet<_> = claim
        .sources()
        .iter()
        .flat_map(|(_, s)| {
            vec![
                s.unique_name(),
                // TODO(andronat): I would like to have a "safer" way to create this names. What if
                // we change these conventions later one?
                format!("{}.{}", s.unique_name(), "verified"),
                format!("{}.{}", s.unique_name(), "uncompressed"),
                format!("{}.{}", s.unique_name(), "uncompressed.verified"),
            ]
        })
        .collect();
    let existing_claim_entries = disk_entries.intersection(&claim_entries);
    let existing_claim_entries_size =
        existing_claim_entries.into_iter().fold(0, |acc, e| {
            let e = dst.as_ref().join(e);
            let size = fs::metadata(&e)
                .map(|meta| meta.len())
                .unwrap_or_else(|err| {
                    warn!(
                        "could not get metadata for `{}`: {}",
                        e.to_string_lossy(),
                        err
                    );
                    0
                });

            acc + size
        });

    if available_space < (claim.full_update_size() - existing_claim_entries_size) {
        warn!(
            "not enough space on disk at `{}`; available space: {}, required space: {}",
            dst.as_ref().display(),
            available_space,
            claim.full_update_size(),
        );
        bail!(
            "something is very wrong here. We can't continue. There is not enough space on disk!"
        );
    }
    Ok(())
}

fn finalize(
    settings: &Settings,
    claim: &Claim,
    version_map: VersionMap,
    version_map_dst: PathBuf,
) -> eyre::Result<()> {
    use orb_update_agent_core::manifest::UpdateKind;

    match claim.manifest().kind() {
        UpdateKind::Full => {
            info!("finalizing full update");
            finalize_full_update(settings, claim, version_map, version_map_dst)
                .wrap_err("failed running full update post update procedures")?;
        }
        UpdateKind::Normal => {
            info!("finalizing normal update");
            finalize_normal_update(settings, claim, version_map, version_map_dst)
                .wrap_err("failed running partial update post update procedures")?;
        }
    }

    info!("rebooting");
    reboot(settings)
}

// Performs post-update logic on a full system update. It currently does not do anything but print
// a message, because it currently relies on the slot switch being induced by a component being
// installed (for example, a component (for example, the smd partition).
fn finalize_full_update(
    settings: &Settings,
    claim: &Claim,
    mut version_map: VersionMap,
    version_map_dst: PathBuf,
) -> eyre::Result<()> {
    info!("finalizing full system update: only updating versions but taking no extra actions");

    version_map.set_recovery_version(claim.version());
    store_version_map_and_legacy(version_map, &version_map_dst, &settings.versions)
        .wrap_err("failed storing versions")?;
    Ok(())
}

// Performs post-update logic on a normal update.
//
// TODO: In the future this also needs to trigger a slot switch for the MCU.
fn finalize_normal_update(
    settings: &Settings,
    claim: &Claim,
    mut version_map: VersionMap,
    version_map_dst: PathBuf,
) -> eyre::Result<()> {
    let target_slot = settings.active_slot.opposite();
    version_map.set_slot_version(claim.version(), target_slot);
    store_version_map_and_legacy(version_map, &version_map_dst, &settings.versions)
        .wrap_err("failed storing versions")?;

    // Set the rootfs status and the boot retry counter for the slot
    slot_ctrl::set_rootfs_status(
        slot_ctrl::RootFsStatus::UpdateDone,
        target_slot.into(),
    )
    .wrap_err_with(|| {
        format!("failed to set the rootfs status for the target slot {target_slot}")
    })?;
    slot_ctrl::reset_retry_count_to_max(target_slot.into()).wrap_err_with(|| {
        format!("failed to set the retry counter for the target slot {target_slot}")
    })?;

    // If a capsule update is scheduled, do not set the next active boot slot
    // The capsule update mechanism will do switch the slot and aplly the update
    if let Ok(efivar) = EfiVar::from_path(EFI_OS_INDICATIONS) {
        match efivar.read() {
            Ok(data) => {
                if data == EFI_OS_REQUEST_CAPSULE_UPDATE.to_vec() {
                    return Ok(());
                }
            }
            Err(_) => {
                warn!("Capsule update was not detected");
            }
        }
    }

    // Set the next active boot slot
    slot_ctrl::set_next_boot_slot(target_slot.into())
        .map(|_| {
            info!("Setting next active slot to slot {target_slot}");
        })
        .wrap_err_with(|| {
            format!("failed to set next active boot slot to slot {target_slot}")
        })
}

fn prepare_environment(settings: &Settings) -> eyre::Result<()> {
    fs::create_dir_all(&settings.workspace).wrap_err_with(|| {
        format!(
            "failed to create download directory and its parents at `{}`",
            settings.workspace.display(),
        )
    })?;
    fs::create_dir_all(&settings.downloads).wrap_err_with(|| {
        format!(
            "failed to create download directory and its parents at `{}`",
            settings.downloads.display(),
        )
    })
}

fn store_version_map_and_legacy(
    map: VersionMap,
    map_dst: &Path,
    legacy_dst: &Path,
) -> eyre::Result<()> {
    serde_json::to_writer(
        &File::options()
            .write(true)
            .read(true)
            .truncate(true)
            .open(map_dst)?,
        &map,
    )
    .wrap_err("saving to version map file failed")?;

    serde_json::to_writer(
        &File::options()
            .write(true)
            .read(true)
            .truncate(true)
            .open(legacy_dst)?,
        &map.to_legacy(),
    )
    .wrap_err("saving to legacy versions file failed")?;

    Ok(())
}

fn shutdown_with_dbus() -> eyre::Result<()> {
    zbus::blocking::Connection::system()
        .wrap_err("failed establishing a `systemd` dbus connection")
        .and_then(|conn| {
            login1::ManagerProxyBlocking::new(&conn)
                .wrap_err("failed creating systemd1 Manager proxy")
        })
        .and_then(|proxy| {
            debug!(
                "scheduling poweroff in 0ms by calling \
                 org.freedesktop.login1.Manager.ScheduleShutdown"
            );
            proxy.schedule_shutdown("poweroff", 0).wrap_err(
                "failed issuing scheduled poweroff to \
                 org.freedesktop.login1.Manager.ScheduleShutdown",
            )
        })
}

fn shutdown_with_executable() -> eyre::Result<()> {
    let output = std::process::Command::new("/bin/systemctl")
        .arg("poweroff")
        .output()
        .wrap_err("failed spawning `/bin/systemctl poweroff`")?;
    ensure!(
        output.status.success(),
        "command `/bin/systemctl poweroff` failed with status code `{:?}` and stderr `{:?}`",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(())
}

/// The microcontroller with a pending update will reboot the Orb when
/// Jetson turns off.
/// ⚠️ BUT, we need to send the power-off/shutdown command to the Jetson
/// because the microcontroller can't detect a Jetson reboot.
fn reboot(settings: &Settings) -> eyre::Result<()> {
    if !settings.recovery && !settings.nodbus {
        debug!("trying to shut down using dbus");
        match shutdown_with_dbus() {
            Ok(()) => return Ok(()),
            Err(e) => {
                error!("error: {e:?}, failed shutting down with systemd dbus call")
            }
        }
    }
    debug!("trying to shut down using executable");
    match shutdown_with_executable() {
        Ok(()) => return Ok(()),
        Err(e) => error!("error: {e:?}, failed shutting down with executable"),
    }
    bail!("shutting down orb failed; see logs for information");
}
