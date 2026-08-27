fn main() {
    orb_build_info::initialize().expect("failed to initialize build info");

    // `dbus`/`zenoh` are off by default (see Cargo.toml). Enable them only on Linux.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!(r#"cargo::rustc-cfg=feature="dbus""#);
        println!(r#"cargo::rustc-cfg=feature="zenoh""#);
    }
}
