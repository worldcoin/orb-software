use std::process::{ExitCode, Termination};

use orb_update_agent::component::Error::{
    self, DownloadRequest, DownloadStatus, InitialLengthRequest, ReadResponse,
    WriteResponse,
};

/// Exit codes returned by the update agent. Custom exit codes are taken in accordance with the
/// Linux Standard Base Core Specification and are in the range 150-199.
#[repr(u8)]
pub(crate) enum UpdateAgentResult {
    Success = 0,
    Failure = 1,
    DownloadFailed = 150,
}

impl Termination for UpdateAgentResult {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

impl From<eyre::Report> for UpdateAgentResult {
    fn from(err: eyre::Report) -> Self {
        use UpdateAgentResult::{DownloadFailed, Failure};
        match err.downcast::<Error>() {
            Ok(
                InitialLengthRequest(..)
                | DownloadRequest { .. }
                | DownloadStatus { .. }
                | ReadResponse { .. }
                | WriteResponse { .. },
            ) => DownloadFailed,
            _ => Failure,
        }
    }
}
