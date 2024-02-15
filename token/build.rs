use anyhow::Result;

fn main() -> Result<()> {
    // Generate vergen's instruction output
    vergen::EmitBuilder::builder()
        .git_sha(false)
        .build_timestamp()
        .emit()
}
