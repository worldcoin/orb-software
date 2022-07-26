fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=../protobuf-definitions/");

    prost_build::Config::new()
        .default_package_filename("mcu_messaging")
        .compile_protos(
            &["../protobuf-definitions/mcu_messaging.proto"],
            &["../protobuf-definitions/"],
        )?;
    Ok(())
}
