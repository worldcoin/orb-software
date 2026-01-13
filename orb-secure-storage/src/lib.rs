#![forbid(unsafe_code)]
//! See [`Client::new()`] as the entrypoint to the api.

pub mod reexported_crates {
    #[cfg(feature = "backend-optee")]
    pub use optee_teec;

    pub use orb_secure_storage_proto;
}

pub mod key;

#[cfg(feature = "backend-optee")]
pub mod optee;

#[cfg(feature = "backend-in-memory")]
pub mod in_memory;

pub use orb_secure_storage_proto::StorageDomain;

use eyre::{Result, WrapErr as _};
use orb_secure_storage_proto::{
    CommandId, GetRequest, PutRequest, RequestT, ResponseT,
};
use rustix::process::Uid;

use crate::key::TryIntoKey;

/// The guts of [`Client`]. It is a trait to allow mocking of the otherwise
/// platform-specific optee calls.
pub trait BackendT {
    type Context;
    type Session: SessionT;
    fn open_session(
        ctx: &mut Self::Context,
        euid: Uid,
        domain: StorageDomain,
    ) -> Result<Self::Session>;
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
    pub fn new(ctx: &mut B::Context, domain: StorageDomain) -> Result<Self> {
        let euid = rustix::process::geteuid();
        let span = tracing::info_span!("orb-secure-storage-client", ?euid);
        let session =
            B::open_session(ctx, euid, domain).wrap_err("failed to create session")?;

        Ok(Self { session, span })
    }

    pub fn get<'a>(&mut self, key: impl TryIntoKey<'a>) -> Result<Vec<u8>> {
        let _span = self.span.enter();
        let key = key.to_key()?;
        let request = GetRequest {
            key: key.as_ref().to_string(),
        };
        let response = invoke_request(&mut self.session, request)?;

        Ok(response.val)
    }

    pub fn put<'a>(
        &mut self,
        key: impl TryIntoKey<'a>,
        value: &[u8],
    ) -> Result<Vec<u8>> {
        let _span = self.span.enter();
        let key = key.to_key()?;
        let request = PutRequest {
            key: key.as_ref().to_owned(),
            val: value.to_owned(),
        };
        let response = invoke_request(&mut self.session, request)?;

        Ok(response.prev_val)
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
    let response_buf = &mut response_buf[0..response_bytes];
    let response = R::Response::deserialize(response_buf)?;

    Ok(response)
}
