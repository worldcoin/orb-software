use std::io::{Read, Seek};

use tracing::info;

pub struct SeekClamp<S> {
    clamp: std::ops::Range<u64>,
    inner: S,
}

impl<S> SeekClamp<S> {
    pub fn new(inner: S, range: impl Into<std::ops::Range<u64>>) -> Self {
        SeekClamp {
            clamp: range.into(),
            inner,
        }
    }
}

impl<S: Seek> Seek for SeekClamp<S> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        info!("seeking to {pos:?}");
        let current = self.inner.stream_position()?;
        if !self.clamp.contains(&current) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "seek would have gone out of bounds",
            ));
        }

        self.inner.seek(pos)
    }
}

impl<R: Read> Read for SeekClamp<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}
