fn main() {
    orb_build_info::initialize().unwrap();
    if let Ok(url) = std::env::var("DEFAULT_ORCHESTRATOR_URL") {
        println!("cargo:rustc-env=DEFAULT_ORCHESTRATOR_URL={url}");
    }
    println!("cargo:rerun-if-env-changed=DEFAULT_ORCHESTRATOR_URL");
}
