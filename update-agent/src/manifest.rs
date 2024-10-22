//! Logic for writing and checking manifests that are found on disk.
use std::{
    fs::{self, File},
    io,
};

use eyre::WrapErr;
use tap::TapFallible as _;
use tracing::{debug, error, info, warn};

extern crate hex;

use std::path::{Path, PathBuf};

use orb_update_agent_core::Manifest;

const MANIFEST_NAME: &str = "manifest.json";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed serializing manifest to json bytes")]
    SerializeManifest(#[from] serde_json::Error),
    #[error("failed writing manifest to `{}`", .0.display())]
    WriteManifest(PathBuf, #[source] io::Error),
}

impl Error {
    fn write_manifest<P: AsRef<Path>>(dst: P, source: io::Error) -> Self {
        Self::WriteManifest(dst.as_ref().to_path_buf(), source)
    }
}

fn read_from_disk<P: AsRef<Path>>(manifest_path: P) -> eyre::Result<Manifest> {
    let manifest_file =
        File::open(&manifest_path).wrap_err("failed opening on-disk manifest")?;
    crate::json::deserialize(manifest_file).wrap_err("failed parsing on-disk manifest")
}

fn write_to_disk<P: AsRef<Path>>(dst: P, manifest: &Manifest) -> Result<(), Error> {
    let bytes = serde_json::to_vec(manifest)?;

    fs::write(&dst, bytes).map_err(|e| Error::write_manifest(dst, e))?;
    Ok(())
}

/// Compares the new manifest against the path we expect an old manifest to be at.
/// If there is not an old manifest, or if the newer manifest is different, then
/// we write the new manifest to disk.
///
/// This allows us to potentially use the last modified time of whe manifest to
/// gauge whether the update is the same in some circumstances (i.e. recovery)
pub fn compare_to_disk<P: AsRef<Path>>(
    new_manifest: &Manifest,
    dir: P,
) -> Result<(), Error> {
    let manifest_path = dir.as_ref().join(MANIFEST_NAME);
    let old_manifest = read_from_disk(&manifest_path)
        .tap_ok(|_| info!("found update manifest at `{}`", manifest_path.display()))
        .tap_err(|e| {
            if matches!(
                e.downcast_ref::<io::Error>().map(io::Error::kind),
                Some(io::ErrorKind::NotFound),
            ) {
                debug!("no old manifest found at `{}`", manifest_path.display());
            } else {
                warn!(
                    "failed reading on-disk manifest on disk at `{}`: {e:?}",
                    manifest_path.display(),
                );
            }
        })
        .ok();

    let should_write_manifest = match old_manifest.as_ref() {
        Some(old_manifest) if old_manifest.is_equivalent_to(new_manifest) => {
            info!("provided manifest and on-disk manifest match");
            false
        }
        Some(_) => {
            info!("mismatch between provided and on-disk manifest; overwriting on-disk manifest");
            true
        }
        None => true,
    };

    if should_write_manifest {
        info!("writing new manifest");
        write_to_disk(&manifest_path, new_manifest)?;
        info!("written manifest to `{}`", manifest_path.display());
    }

    Ok(())
}
