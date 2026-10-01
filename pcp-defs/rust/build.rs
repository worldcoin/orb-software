use std::{env, io::Result, path::PathBuf};

fn main() -> Result<()> {
    let proto_root = "./proto";
    let proto_files = [
        "./proto/pcp/v1/backend_keys.proto",
        "./proto/pcp/v1/di_iris_embeddings.proto",
        "./proto/pcp/v1/di_iris_embedding_shares.proto",
        "./proto/pcp/v1/face_embeddings.proto",
        "./proto/pcp/v1/hashes.proto",
        "./proto/pcp/v1/info.proto",
        "./proto/pcp/v1/iris_code_shares.proto",
        "./proto/pcp/v1/iris_codes.proto",
    ];

    for f in &proto_files {
        println!("cargo:rerun-if-changed={f}");
    }
    println!("cargo:rerun-if-changed={proto_root}");
    println!("cargo:rerun-if-changed=build.rs");

    let descriptor_path =
        PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"))
            .join("descriptor.bin");
    prost_build::Config::new()
        .file_descriptor_set_path(&descriptor_path)
        .compile_protos(&proto_files, &[proto_root])?;

    pbjson_build::Builder::new()
        .register_descriptors(&std::fs::read(&descriptor_path)?)?
        .emit_fields()
        // hashes.json carries per-frame keys that no proto field can name.
        .ignore_unknown_fields()
        .build(&[".pcp"])?;

    Ok(())
}
