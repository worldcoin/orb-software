#![forbid(unsafe_code)]
//! See [`Client::new()`] as the entrypoint to the api.

pub mod reexported_crates {
    #[cfg(feature = "backend-optee")]
    pub use optee_teec;

    pub use orb_camera_hmac_proto;
}

#[cfg(feature = "backend-optee")]
pub mod optee;

use eyre::{Result, WrapErr as _};
use orb_camera_hmac_proto::{
    CommandId, GetRowStartRequest, GetRowStartResponse, ProvisionKeyRequest,
    RequestT, ResponseT, VersionRequest, VerifyHmacRequest, VerifyHmacResponse,
};
use rustix::process::Uid;
use tracing::info;

/// The guts of [`Client`]. It is a trait to allow mocking of the otherwise
/// platform-specific optee calls.
pub trait BackendT {
    type Context;
    type Session: SessionT;
    fn open_session(ctx: &mut Self::Context, euid: Uid) -> Result<Self::Session>;
}

/// The session returned by [`BackendT::open_session`].
pub trait SessionT {
    fn invoke(
        &mut self,
        command: CommandId,
        serialized_request: &[u8],
        response_buf: &mut [u8],
    ) -> Result<usize>;
}

/// The entrypoint of the API.
///
/// For the choice of `B`, typically you use [`crate::optee::OpteeBackend`] (except
/// in tests).
pub struct Client<B: BackendT> {
    session: B::Session,
    span: tracing::Span,
}

impl<B: BackendT> Client<B> {
    pub fn new(ctx: &mut B::Context) -> Result<Self> {
        let euid = rustix::process::geteuid();
        let span = tracing::info_span!("orb-camera-hmac-client", ?euid);
        let session =
            B::open_session(ctx, euid).wrap_err("failed to create session")?;

        let mut self_ = Self { session, span };

        let ta_version = self_.version().wrap_err("failed to request TA version")?;
        info!("got orb-camera-hmac-ta version: {ta_version}");

        Ok(self_)
    }

    /// Store the 32-byte PPK inside OPTEE secure storage.
    pub fn provision_key(&mut self, ppk: [u8; 32]) -> Result<()> {
        let _span = self.span.enter();
        let request = ProvisionKeyRequest { ppk };
        invoke_request(&mut self.session, request)
            .wrap_err("failed to invoke ProvisionKeyRequest")?;
        Ok(())
    }

    /// Derive the per-frame row-start index SR2H.
    pub fn get_row_start(
        &mut self,
        request: GetRowStartRequest,
    ) -> Result<GetRowStartResponse> {
        let _span = self.span.enter();
        invoke_request(&mut self.session, request)
            .wrap_err("failed to invoke GetRowStartRequest")
    }

    /// Verify the HMAC embedded in a camera frame.
    pub fn verify_hmac(
        &mut self,
        request: VerifyHmacRequest,
    ) -> Result<VerifyHmacResponse> {
        let _span = self.span.enter();
        invoke_request(&mut self.session, request)
            .wrap_err("failed to invoke VerifyHmacRequest")
    }

    pub fn version(&mut self) -> Result<String> {
        let _span = self.span.enter();
        let request = VersionRequest;
        let response = invoke_request(&mut self.session, request)
            .wrap_err("failed to invoke VersionRequest")?;
        Ok(response.0)
    }
}

fn invoke_request<R: RequestT>(
    session: &mut impl SessionT,
    request: R,
) -> Result<R::Response> {
    let mut response_buf = vec![0; R::MAX_RESPONSE_SIZE as usize];
    let serialized_request = serde_json::to_vec(&request).expect("infallible");

    let response_bytes = session
        .invoke(request.id(), &serialized_request, &mut response_buf)
        .wrap_err("failed to invoke optee command")?;
    let response_buf = &response_buf[..response_bytes];
    let response = R::Response::deserialize(response_buf)?;

    Ok(response)
}
