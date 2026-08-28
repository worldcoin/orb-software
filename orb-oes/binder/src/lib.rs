//! Binder proxy and interface for `orb-oes`.
//!
//! Two AIDL interfaces are generated here:
//! - `IAuthTokenManager` (`aidl/org/worldcoin/attest/IAuthTokenManager.aidl`,
//!   the same interface `orb-attest` registers as a service): consumed here
//!   as a client to fetch the current backend auth token.
//! - `IOesEventStream` (`aidl/org/worldcoin/oes/IOesEventStream.aidl`, new):
//!   registered here as a service so other on-device processes can push OES
//!   events in, following the same injected-impl shape as
//!   `orb-attest-binder`'s `AuthTokenManager<T>`.
//!
//! Each generated interface is wrapped in its own private module: two
//! `rsbinder::include_aidl!` calls at the same scope would both try to
//! define `pub mod org`, since the shared `org.worldcoin` package prefix is
//! emitted verbatim by each generated file.

mod auth_token_manager_gen {
    rsbinder::include_aidl!(
        "auth_token_manager",
        crate::auth_token_manager_gen::org::worldcoin::attest::IAuthTokenManager::*
    );
}
mod oes_event_stream_gen {
    rsbinder::include_aidl!(
        "oes_event_stream",
        crate::oes_event_stream_gen::org::worldcoin::oes::IOesEventStream::*
    );
}

pub use auth_token_manager_gen::IAuthTokenManager;
pub use oes_event_stream_gen::{BnOesEventStream, IOesEventStream};

/// Well-known name `IAuthTokenManager` is registered under by `orb-attest`.
const AUTH_TOKEN_MANAGER_SERVICE_NAME: &str = "org.worldcoin.AuthTokenManager";

/// Fetch the current backend auth token from `orb-attest`'s binder service.
///
/// # Errors
/// - the service isn't registered yet / the binder driver can't be reached
/// - `orb-attest` hasn't fetched a token from the backend yet
pub fn get_auth_token() -> rsbinder::BinderResult<String> {
    let manager: rsbinder::Strong<dyn IAuthTokenManager> =
        rsbinder::hub::wait_for_interface(AUTH_TOKEN_MANAGER_SERVICE_NAME)
            .map_err(rsbinder::Status::from)?;

    manager.getToken()
}

pub trait OesEventStreamT: Send + Sync + 'static {
    fn push_event(&self, name: String, payload_json: String, mode: i32);
}

#[derive(derive_more::From)]
pub struct OesEventStream<T>(pub T);

impl<T: OesEventStreamT> rsbinder::Interface for OesEventStream<T> {}

impl<T: OesEventStreamT> IOesEventStream for OesEventStream<T> {
    #[allow(non_snake_case)]
    fn pushEvent(
        &self,
        name: &str,
        payloadJson: &str,
        mode: i32,
    ) -> rsbinder::BinderResult<()> {
        self.0
            .push_event(name.to_owned(), payloadJson.to_owned(), mode);

        Ok(())
    }
}
