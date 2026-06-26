pub mod claim;
pub mod client;
pub mod component;
pub mod dbus;
pub mod json;
pub mod manifest;
pub mod mount;
pub mod settings;
pub mod update;
pub mod util;

use component::Component;
use eyre::{ensure, WrapErr as _};
use orb_build_info::{make_build_info, BuildInfo};
use orb_update_agent_core::{Slot, VersionMap};
pub use settings::{Args, Settings};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::ExitCode,
};

pub const BUILD_INFO: BuildInfo = make_build_info!();

mod exit_codes {
    pub const DOWNLOAD_FAILED: u8 = 150;
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("OrbInfo os-release: {0}")]
    OrbOsRelease(#[from] orb_info::orb_os_release::ReadErr),
    #[error("SlotControl: {0}")]
    SlotControl(#[from] orb_slot_ctrl::Error),
    #[error("Settings: {0}")]
    Settings(#[from] figment::Error),
    #[error("ReadingVersions: {0}")]
    ReadingVersions(eyre::Report),
    #[error("Claim: {0}")]
    Claim(#[from] claim::Error),
    #[error("Component: {0}")]
    Component(#[from] component::Error),
    #[error("FetchUpdateComponents: {0}")]
    FetchUpdateComponents(eyre::Report),
    #[error("Supervisor: {0}")]
    Supervisor(eyre::Report),
    #[error("RunUpdate: {0}")]
    RunUpdate(eyre::Report),
    #[error("UpdateComponentVersionsOnDisk: {0}")]
    UpdateComponentVersionOnDisk(eyre::Report),
    #[error("CopyRedundantComponents: {0}")]
    CopyRedundantComponents(eyre::Report),
    #[error("Finalize: {0}")]
    Finalize(eyre::Report),
    #[error("Other {0}")]
    Other(eyre::Report),
}

impl From<Error> for ExitCode {
    fn from(val: Error) -> Self {
        use component::Error::*;

        match val {
            Error::Component(
                RangeRequest(..)
                | InitialLengthRequest(..)
                | ResponseStatus(..)
                | GetBytes(..),
            ) => ExitCode::from(exit_codes::DOWNLOAD_FAILED),

            _ => ExitCode::FAILURE,
        }
    }
}

impl Error {
    pub fn to_dd_tag(&self) -> &'static str {
        match self {
            Error::OrbOsRelease(_) => "orb-info-os-release",
            Error::SlotControl(_) => "slot-control",
            Error::Settings(_) => "settings",
            Error::ReadingVersions(_) => "reading-versions",
            Error::Other(_) => "other",
            Error::FetchUpdateComponents(_) => "fetch-update-components",
            Error::Supervisor(_) => "supervisor",
            Error::RunUpdate(_) => "run-update",
            Error::UpdateComponentVersionOnDisk(_) => {
                "update-component-version-on-disk"
            }
            Error::CopyRedundantComponents(_) => "copy-redundant-components",
            Error::Finalize(_) => "finalize",

            Error::Claim(error) => {
                use claim::Error::*;
                match error {
                    ReadJson(_) => "claim-read-json",
                    OpenPath { .. } => "claim-open-path",
                    InitClient(..) => "claim-init-client",
                    DBusToken(..) => "claim-dbus-token",
                    DBusTokenNotAvailable(_) => "claim-dbus-token-not-avaialble",
                    NoAuthTokenProvided() => "claim-no-auth-token-provided",
                    SendCheckUpdateRequest(..) => "claim-send-check-update-request",
                    ResponseAsText(_) => "claim-response-as-text",
                    Local { .. } => "claim-local",
                    Remote { .. } => "claim-remote",
                    DbusRequest(_) => "claim-dbus-request",
                    DownloadNotAllowed { .. } => "claim-download-not-allowed",
                    StatusCode { .. } => "claim-status-code",
                    MissingSlotVersion { .. } => "claim-missing-slot-version",
                    NoNewVersion => "claim-no-new-version",
                    Validation(_) => "claim-validation",
                }
            }

            Error::Component(error) => {
                use component::Error::*;
                match error {
                    InitClient(_) => "component-init-client",
                    ClaimSizeRemoteLenMismatch(_, _, _) => {
                        "component-claim-size-remote-len-mismatch"
                    }
                    MissingContentLengthHeader(_) => {
                        "claim-missing-content-length-header"
                    }
                    NonStringContentLengthValue(..) => {
                        "claim-non-string-content-length-value"
                    }
                    InvalidContentLengthValue(..) => {
                        "claim-invalid-content-length-value"
                    }
                    OpenWriteTarget(..) => "claim-open-write-target",
                    InvalidHttpRange(..) => "claim-invalid-http-range",
                    RangeRequest(..) => "claim-range-request",
                    InitialLengthRequest(..) => "claim-initial-lenght-request",
                    ResponseStatus(..) => "claim-response-status",
                    GetBytes(..) => "claim-get-bytes",
                    MergeChunk(..) => "claim-merge-chunk",
                    HashMismatch { .. } => "claim-hash-mismatch",
                    DiskSync(..) => "claim-disk-sync",
                    MimeUnknown { .. } => "claim-mime-unknown",
                }
            }
        }
    }
}

/// Writes a serializable value as JSON to the given path and syncs to disk.
pub fn write_json_and_sync(
    path: &Path,
    value: &impl serde::Serialize,
) -> eyre::Result<()> {
    let file = File::options()
        .write(true)
        .read(true)
        .create(true)
        .truncate(true)
        .open(path)
        .wrap_err_with(|| format!("failed to open `{}`", path.display()))?;
    serde_json::to_writer(&file, value)
        .wrap_err_with(|| format!("failed to write JSON to `{}`", path.display()))?;
    file.sync_all()
        .wrap_err_with(|| format!("failed to sync `{}` to disk", path.display()))?;
    Ok(())
}

pub fn update_component_version_on_disk(
    target_slot: Slot,
    component: &Component,
    version_map: &mut VersionMap,
    path: &Path,
) -> eyre::Result<()> {
    version_map.set_component(
        target_slot,
        component.manifest_component(),
        component.system_component(),
    );
    write_json_and_sync(path, &version_map)
}

/// After confirming reads work at the extremeties of the given range, this function
/// will seek to `range.start`.
fn confirm_read_works_at_bounds(
    mut f: impl Read + Seek,
    range: std::ops::Range<u64>,
) -> eyre::Result<()> {
    let len = f
        .seek(SeekFrom::End(0))
        .wrap_err("failed to seek to End(0)")?;
    ensure!(
        range.end <= len,
        "range end {} was out of bounds of seek length {}",
        range.end,
        len
    );

    f.seek(SeekFrom::Start(range.start))
        .wrap_err_with(|| format!("failed to seek to `range.start` {}", range.start))?;
    f.read_exact(&mut [0; 1])
        .wrap_err_with(|| format!("failed to read at `range.start` {}", range.start))?;
    f.seek(SeekFrom::Start(range.end - 1)).wrap_err_with(|| {
        format!("failed to seek to `range.end-1` {}", range.end - 1)
    })?;
    f.read_exact(&mut [0; 1]).wrap_err_with(|| {
        format!("failed to read at `range.end-1` {}", range.end - 1)
    })?;
    f.seek(SeekFrom::Start(range.start)).wrap_err_with(|| {
        format!("failed to return to `range.start` {}", range.start)
    })?;

    Ok(())
}

/// Re-export commonly used types for convenience
pub use dbus::interfaces::UpdateProgress as UpdateAgentProgress;
pub use orb_update_agent_dbus::{ComponentState, ComponentStatus, UpdateAgentState};
