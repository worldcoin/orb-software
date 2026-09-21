//! In-memory tar and gzip encoding for PCP payloads and tiers.
//!
//! Returned buffers are not encrypted or automatically zeroized. Callers own
//! their sensitive-data handling; compression does not protect confidentiality.

use std::{collections::BTreeSet, io::Write};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid archive filename")]
    InvalidName,
    #[error("duplicate archive filename")]
    DuplicateName,
    #[error("timestamp exceeds the gzip 32-bit seconds field")]
    TimestampOutOfRange,
    #[error("archive encoding failed")]
    Io(#[from] std::io::Error),
}

/// Encodes regular files in the supplied order using the PCP GNU tar headers.
///
/// The timestamp is seconds since the Unix epoch, shared by all entries.
/// Headers use uid/gid 0, mode 0644, and device major/minor 0. Input buffers
/// are borrowed; the returned archive owns a copy of their bytes.
///
/// Names must be nonempty single components, at most 100 UTF-8 bytes, without
/// `/`, `\`, `:`, or control characters; `.` and `..` are rejected. Duplicate
/// names are rejected. The package layer must choose the protocol filenames
/// and entry order; this helper does not enforce a complete package layout.
///
/// ```
/// use orb_pcp::archive::{compress, encode_tar};
///
/// let tar = encode_tar(123, [("example.bin", b"example".as_slice())])?;
/// let gzip = compress(&tar, 123, "tier0.tar.gz")?;
/// # Ok::<(), orb_pcp::archive::Error>(())
/// ```
pub fn encode_tar<'a>(
    timestamp: u64,
    entries: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<Vec<u8>, Error> {
    let mut archive = tar::Builder::new(Vec::new());
    let mut names = BTreeSet::new();
    for (name, data) in entries {
        validate_name(name)?;
        if !names.insert(name) {
            return Err(Error::DuplicateName);
        }
        // Preserve the GNU header's NUL regular-file type for byte compatibility.
        let mut header = tar::Header::new_gnu();
        header.set_path(name)?;
        header.set_size(data.len() as u64);
        header.set_mtime(timestamp);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mode(0o644);
        header.set_device_major(0)?;
        header.set_device_minor(0)?;
        header.set_cksum();
        archive.append(&header, data)?;
    }
    Ok(archive.into_inner()?)
}

/// Gzip-encodes bytes at the best compression level with explicit header metadata.
///
/// The timestamp is seconds since the Unix epoch and must fit in `u32`.
/// The filename follows the same rules as [`encode_tar`]. PCP tier filenames
/// are `tier0.tar.gz`, `tier1.tar.gz`, and `tier2.tar.gz`; inner archives remain
/// uncompressed. Compressed bytes may vary with the compression backend/version;
/// the header metadata and decompressed bytes are the compatibility contract.
pub fn compress(data: &[u8], timestamp: u64, filename: &str) -> Result<Vec<u8>, Error> {
    validate_name(filename)?;
    let timestamp = u32::try_from(timestamp).map_err(|_| Error::TimestampOutOfRange)?;
    let mut encoder = flate2::GzBuilder::new()
        .filename(filename)
        .mtime(timestamp)
        .write(Vec::new(), flate2::Compression::best());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

fn validate_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > 100
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    {
        return Err(Error::InvalidName);
    }
    Ok(())
}
