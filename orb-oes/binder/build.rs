use std::path::PathBuf;

fn main() {
    rsbinder_aidl::Builder::new()
        .source(PathBuf::from(
            "aidl/org/worldcoin/attest/IAuthTokenManager.aidl",
        ))
        .output(PathBuf::from("auth_token_manager.rs"))
        .set_async_support(false)
        .generate()
        .unwrap();

    rsbinder_aidl::Builder::new()
        .source(PathBuf::from("aidl/org/worldcoin/oes/IOesEventStream.aidl"))
        .output(PathBuf::from("oes_event_stream.rs"))
        .set_async_support(false)
        .generate()
        .unwrap();
}
