use tempfile::TempDir;

#[derive(Debug, bon::Builder)]
pub struct Harness {
    #[builder(default = TempDir::new().expect("failed to create tempdir"))]
    tempdir: tempfile::TempDir,
}
