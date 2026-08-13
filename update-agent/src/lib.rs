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

use eyre::{ensure, WrapErr as _};
use orb_build_info::{make_build_info, BuildInfo};
pub use settings::{Args, Settings};
use std::{
    borrow::Cow,
    io::{Read, Seek, SeekFrom},
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
    #[error("Claim: {0}")]
    Claim(#[from] claim::Error),
    #[error("Component: {0}")]
    Component(#[from] component::Error),
    #[error("Manifest: {0}")]
    Manifest(#[from] manifest::Error),
    #[error("Supervisor: {0}")]
    Supervisor(eyre::Report),
    #[error("RunUpdate: {0}")]
    RunUpdate(eyre::Report),
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
    pub fn to_dd_tag(&self) -> Cow<'static, str> {
        match self {
            Error::OrbOsRelease(_) => "orb-info-os-release".into(),
            Error::SlotControl(_) => "slot-control".into(),
            Error::Settings(_) => "settings".into(),
            Error::Other(_) => "other".into(),
            Error::Manifest(_) => "manifest".into(),
            Error::Supervisor(_) => "supervisor".into(),
            Error::RunUpdate(_) => "run-update".into(),
            Error::CopyRedundantComponents(_) => "copy-redundant-components".into(),
            Error::Finalize(_) => "finalize".into(),
            Error::Claim(error) => error.to_dd_tag(),
            Error::Component(error) => error.to_dd_tag().into(),
        }
    }
}

impl claim::Error {
    pub fn to_dd_tag(&self) -> Cow<'static, str> {
        use claim::Error::*;
        match self {
            ReadJson(_) => "claim-read-json".into(),
            OpenPath { .. } => "claim-open-path".into(),
            InitClient(..) => "claim-init-client".into(),
            DBusToken(..) => "claim-dbus-token".into(),
            DBusTokenNotAvailable(_) => "claim-dbus-token-not-avaialble".into(),
            NoAuthTokenProvided() => "claim-no-auth-token-provided".into(),
            SendCheckUpdateRequest(..) => "claim-send-check-update-request".into(),
            ResponseAsText(_) => "claim-response-as-text".into(),
            Local { source, .. } => match source.as_ref() {
                Local { .. } => "claim-local".into(),
                Remote { .. } => "claim-remote-local".into(),
                _ => format!("{}-local", source.to_dd_tag()).into(),
            },
            Remote { source, .. } => match source.as_ref() {
                Local { .. } => "claim-local-remote".into(),
                Remote { .. } => "claim-remote".into(),
                _ => format!("{}-remote", source.to_dd_tag()).into(),
            },
            DbusRequest(_) => "claim-dbus-request".into(),
            DownloadNotAllowed { .. } => "claim-download-not-allowed".into(),
            StatusCode { .. } => "claim-status-code".into(),
            NoNewVersion => "claim-no-new-version".into(),
        }
    }
}

impl component::Error {
    pub fn to_dd_tag(&self) -> &'static str {
        use component::Error::*;
        match self {
            InitClient(_) => "component-init-client",
            ClaimSizeRemoteLenMismatch(_, _, _) => {
                "component-claim-size-remote-len-mismatch"
            }
            MissingContentLengthHeader(_) => "component-missing-content-length-header",
            NonStringContentLengthValue(..) => {
                "component-non-string-content-length-value"
            }
            InvalidContentLengthValue(..) => "component-invalid-content-length-value",
            OpenWriteTarget(..) => "component-open-write-target",
            InvalidHttpRange(..) => "component-invalid-http-range",
            RangeRequest(..) => "component-range-request",
            InitialLengthRequest(..) => "component-initial-lenght-request",
            ResponseStatus(..) => "component-response-status",
            GetBytes(..) => "component-get-bytes",
            MergeChunk(..) => "component-merge-chunk",
            HashMismatch { .. } => "component-hash-mismatch",
            DiskSync(..) => "component-disk-sync",
            MimeUnknown { .. } => "component-mime-unknown",
            Process(..) => "component-process",
        }
    }
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
