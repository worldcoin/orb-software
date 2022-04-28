use std::io::Result;

use prost_build;

fn main() -> Result<()> {
    prost_build::Config::default()
        .bytes(&["."])
        .compile_protos(&["protos/container.proto"], &["protos/"])?;
    Ok(())
}
