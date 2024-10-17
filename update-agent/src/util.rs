use core::fmt::Formatter;
use std::{
    fs::File,
    io::copy,
    path::{Path, PathBuf},
};

use eyre::{bail, WrapErr as _};
use sha2::{Digest as _, Sha256};

pub fn check_hash<P: AsRef<Path>>(
    path_to_blob: P,
    expected_hex_hash: &str,
) -> eyre::Result<()> {
    let display_path = path_to_blob.as_ref().display();
    let decoded_hash = hex::decode(expected_hex_hash).wrap_err_with(|| {
        format!("failed to decode hex string as hash: {expected_hex_hash}")
    })?;
    let mut hasher = Sha256::new();
    let mut blob = File::open(&path_to_blob)
        .wrap_err_with(|| format!("failed opening `{display_path}` for hashing"))?;
    copy(&mut blob, &mut hasher)
        .wrap_err("failed to copy component blob into hasher")?;
    let result = hasher.finalize();
    if *result != decoded_hash {
        let encoded_result = hex::encode(result);
        bail!(
            "mismatch between recorded and actual hashes of `{display_path}`; expected \
             `{expected_hex_hash}`, calculated `{encoded_result}`"
        );
    }
    Ok(())
}

/// Component file name in form of "{parent}/{name}-{hash}"
///
/// Example: ```downloads/rootfs-bc4c24181ed3ce6666444deeb95e1f61940bffee70dd13972beb331f5d111e9b```
pub fn make_component_path<P: AsRef<Path>>(parent: P, unique_name: &str) -> PathBuf {
    parent.as_ref().join(unique_name)
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid http range parameters: start must not exceed end, buffer size must be non-zero")]
pub struct HttpRangeError {
    pub start: u64,
    pub end: u64,
    pub buffer_size: u32,
}

pub struct HttpRangeIter {
    start: u64,
    end: u64,
    buffer_size: u32,
}

impl HttpRangeIter {
    pub fn try_new(
        start: u64,
        end: u64,
        buffer_size: u32,
    ) -> Result<HttpRangeIter, HttpRangeError> {
        if buffer_size < 1 || start > end {
            return Err(HttpRangeError {
                start,
                end,
                buffer_size,
            });
        }
        Ok(HttpRangeIter {
            start,
            end,
            buffer_size,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Range {
    start: u64,
    end: u64,
    total_size: u64,
}

impl Iterator for HttpRangeIter {
    type Item = Range;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start > self.end {
            None
        } else {
            let prev_start = self.start;
            // Handle final chunk
            self.start +=
                std::cmp::min(self.buffer_size as u64, self.end - self.start + 1);
            // For `Range` HTTP request header documentation, see:
            // https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Range
            let end = self.start - 1;
            Some(Range {
                start: prev_start,
                end,
                total_size: self.end,
            })
        }
    }
}

impl std::fmt::Display for Range {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            f.write_fmt(format_args!("{}%", self.start * 100 / self.total_size))
        } else {
            f.write_fmt(format_args!("bytes={}-{}", self.start, self.end))
        }
    }
}
