use std::io::Result;

fn main() -> Result<()> {
    let proto_root = "./proto";
    let proto_files = [
        "./proto/pcp/v1/di_iris_embeddings.proto",
        "./proto/pcp/v1/di_iris_embedding_shares.proto",
    ];

    for f in &proto_files {
        println!("cargo:rerun-if-changed={f}");
    }
    println!("cargo:rerun-if-changed={proto_root}");
    println!("cargo:rerun-if-changed=build.rs");

    prost_build::Config::new().compile_protos(&proto_files, &[proto_root])?;

    Ok(())
}
