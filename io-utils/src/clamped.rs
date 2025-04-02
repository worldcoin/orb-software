use std::io::{Error, ErrorKind, Read, Result, Seek, SeekFrom};

/// Clamps a [`Read + Seek`] to ensure that it does not read out of the clamped range.
///
/// Seeks are not themselves clamped, only the final read - this is analogous to how
/// [`std::fs::File`] works.
pub struct ClampedSeek<S> {
    /// The starting stream position of `inner` at time of construction.
    start: u64,
    /// relative to `start`
    clamp: std::ops::RangeTo<u64>,
    /// relative to `start`
    cursor: u64,
    inner: S,
}

impl<S: Seek> ClampedSeek<S> {
    /// Constructs a new `ClampedSeek`. The initial position of `inner` will become the
    /// new 0 position of `Self` and it won't be possible to seek or read before that.
    ///
    /// `range` is relative to `innner`'s starting position.
    pub fn new(mut inner: S, range: impl Into<std::ops::RangeTo<u64>>) -> Result<Self> {
        let clamp = range.into();
        let start = inner.stream_position()?;

        Ok(ClampedSeek {
            clamp,
            start,
            cursor: 0,
            inner,
        })
    }
}

impl<S: Seek> Seek for ClampedSeek<S> {
    /// This seek implementation returns `UnexpectedEof` when reading past the end of
    /// the clamped range, and `InvalidInput` when reading before it.
    ///
    /// This is analagous to how [`std::fs::File`] behaves.
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let from_self_start: u64 = match pos {
            SeekFrom::Start(from_start) => from_start,
            SeekFrom::End(from_end) if from_end.is_negative() => {
                self.clamp.end.checked_add_signed(from_end).ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidInput,
                        format!(
                            "attempted to seek before offset 0: {} + {pos:?}",
                            self.clamp.end
                        ),
                    )
                })?
            }
            SeekFrom::End(from_end) => {
                assert!(from_end.is_positive() || from_end == 0, "sanity");
                self.clamp.end.checked_add_signed(from_end).ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidInput,
                        format!("arithmetic overflow: {} + {pos:?}", self.clamp.end),
                    )
                })?
            }
            SeekFrom::Current(from_current) if from_current.is_negative() => self
                .cursor
                .checked_add_signed(from_current)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidInput,
                        format!(
                            "attempted to seek before offset 0: {} + {pos:?}",
                            self.cursor
                        ),
                    )
                })?,
            SeekFrom::Current(from_current) => {
                assert!(from_current.is_positive() || from_current == 0, "sanity");
                self.cursor
                    .checked_add_signed(from_current)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::InvalidInput,
                            format!("arithmetic overflow: {} + {pos:?}", self.cursor),
                        )
                    })?
            }
        };

        let from_inner_start =
            self.start.checked_add(from_self_start).ok_or_else(|| {
                Error::new(
                    ErrorKind::InvalidInput,
                    format!("arithmetic overflow: {} + {from_self_start}", self.start),
                )
            })?;

        let inner_cursor = self.inner.seek(SeekFrom::Start(from_inner_start))?;
        assert_eq!(
            inner_cursor, from_inner_start,
            "inner cursor doesn't match ClampedSeek cursor, this is a bug"
        );
        self.cursor = from_self_start;

        Ok(self.cursor)
    }
}

impl<RS: Read + Seek> Read for ClampedSeek<RS> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cursor >= self.clamp.end {
            // short circuit: we are clamped
            return Ok(0);
        }

        let max_bytes = self
            .clamp
            .end
            .checked_sub(self.cursor)
            .expect("infallible: cursor is smaller");
        let max_bytes = usize::try_from(max_bytes).expect("overflow");
        let max_bytes = max_bytes.min(buf.len());

        let n_bytes_read = self.inner.read(&mut buf[..max_bytes])?;
        assert!(n_bytes_read <= max_bytes, "sanity");
        self.cursor += u64::try_from(n_bytes_read).expect("overflow");

        Ok(n_bytes_read)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::{Cursor, ErrorKind};

    fn all_possible_valid_absolute_seeks(
        len: u64,
    ) -> impl Iterator<Item = SeekFrom> + Clone {
        let len_i64 = i64::try_from(len).expect("overflow");

        let from_start = (0..len).map(SeekFrom::Start);

        let from_end = (0..len_i64).map(move |idx| -1 - idx).map(SeekFrom::End);

        from_start.chain(from_end)
    }

    fn all_possible_valid_relative_seeks(
        current: u64,
        len: u64,
    ) -> impl Iterator<Item = SeekFrom> + Clone {
        let current_i64 = i64::try_from(current).expect("overflow");
        let len_i64 = i64::try_from(len).expect("overflow");

        let lower = (0..current_i64).map(|abs_idx| SeekFrom::Current(-abs_idx));
        let upper = (current_i64..len_i64)
            .map(move |abs_idx| SeekFrom::Current(abs_idx - current_i64));

        lower.chain(upper)
    }

    fn read_1(mut read: impl Read) -> std::io::Result<u8> {
        let buf = &mut [0u8; 1];
        read.read_exact(buf)?;
        Ok(buf[0])
    }

    #[test]
    fn document_behavior_of_cursor() {
        // This test exists to self-document the behavior of std::io::Cursor, since
        // it can be hard sometimes to know how it works.

        // Arrange
        let test_data: Vec<_> = (0..8).collect();
        assert_eq!(test_data.len(), 8, "sanity");
        assert_eq!(test_data[0], 0, "sanity");
        assert_eq!(test_data[7], 7, "sanity");
        assert!(test_data.get(8).is_none(), "sanity");

        let mut cursor = Cursor::new(test_data);

        // Act + Assert

        // indices [0,7] are valid for SeekFrom::Start
        for from_start in 0..=7u64 {
            assert_eq!(
                cursor.seek(SeekFrom::Start(from_start)).unwrap(),
                from_start
            );
            assert_eq!(u64::from(read_1(&mut cursor).unwrap()), from_start);
        }

        // index 8 is invalid for SeekFrom::Start
        assert_eq!(
            cursor
                .seek(SeekFrom::Start(8))
                .expect("index out of bounds but is still OK"),
            8
        );
        assert!(read_1(&mut cursor).is_err(), "only the read fails");

        // indicies [-8..-1] are valid for SeekFrom::End
        for from_end in -8..=-1 {
            let from_start = u64::try_from(8 + from_end).unwrap();
            assert_eq!(cursor.seek(SeekFrom::End(from_end)).unwrap(), from_start);
            assert_eq!(u64::from(read_1(&mut cursor).unwrap()), from_start);
        }

        // index 0 is invalid for SeekFrom::End
        assert_eq!(
            cursor
                .seek(SeekFrom::End(0))
                .expect("index out of bounds but is still OK"),
            8
        );
        assert!(read_1(&mut cursor).is_err(), "only the read fails");

        // index 1 is invalid for SeekFrom::End
        assert_eq!(
            cursor
                .seek(SeekFrom::End(1))
                .expect("index out of bounds but is still OK"),
            9
        );
        assert!(read_1(&mut cursor).is_err(), "only the read fails");
    }

    #[test]
    fn test_seek_and_read_just_before_end() -> Result<()> {
        use SeekFrom::*;

        // Arrange
        let mut cursor = Cursor::new((0..6u8).collect::<Vec<_>>());
        cursor.seek(Start(2))?;
        let mut clamped = ClampedSeek::new(cursor, ..4)?;

        // Act + assert seek to last byte
        assert_eq!(clamped.seek(End(-1))?, 3);
        assert_eq!(clamped.cursor, 3);
        assert_eq!(clamped.inner.stream_position()?, 5);

        // read gives 1 byte
        let buf = &mut [0; 1];
        assert_eq!(clamped.read(buf)?, 1, "1 byte should be read");
        assert_eq!(
            buf[0], 5,
            "element at 0-indexed 5th index is 5 because we started at 2"
        );
        assert_eq!(clamped.cursor, 4);
        assert_eq!(clamped.inner.stream_position()?, 6);

        // read gives 0 bytes
        assert_eq!(clamped.read(buf)?, 0, "0 bytes should be read");
        assert_eq!(clamped.cursor, 4, "cursor shouldn't advance");
        assert_eq!(
            clamped.inner.stream_position()?,
            6,
            "inner cursor shouldn't advance"
        );

        Ok(())
    }

    #[test]
    fn test_seek_and_read_at_end() -> Result<()> {
        use SeekFrom::*;

        // Arrange
        let mut cursor = Cursor::new((0..6u8).collect::<Vec<_>>());
        cursor.seek(Start(2))?;
        let mut clamped = ClampedSeek::new(cursor, ..4)?;

        // Act + assert seek to end
        assert_eq!(clamped.seek(End(0))?, 4);
        assert_eq!(clamped.cursor, 4);
        assert_eq!(clamped.inner.stream_position()?, 6);

        // read gives 0 bytes
        let buf = &mut [0; 1];
        assert_eq!(clamped.read(buf)?, 0, "0 bytes should be read");
        assert_eq!(clamped.cursor, 4, "cursor shouldn't advance");
        assert_eq!(
            clamped.inner.stream_position()?,
            6,
            "inner cursor shouldn't advance"
        );

        Ok(())
    }

    #[test]
    fn ensure_clamped_and_unclamped_give_same_output_when_seeking_in_valid_range() {
        // Arrange
        let test_range = 0..8u8;
        let test_data: Vec<_> = test_range.clone().collect();
        assert_eq!(test_data.len(), 8, "sanity");
        assert_eq!(test_data[0], 0, "sanity");
        assert_eq!(test_data[7], 7, "sanity");
        assert!(test_data.get(8).is_none(), "sanity");

        let mut unclamped = Cursor::new(test_data.clone());
        let mut clamped = ClampedSeek::new(unclamped.clone(), ..8).unwrap();

        let all_absolute_seeks = all_possible_valid_absolute_seeks(8);
        assert_eq!(
            all_absolute_seeks.clone().count(),
            16,
            "sanity: 8 from start, 8 from end"
        );

        // Act + assert
        //
        for seek_from in all_absolute_seeks {
            println!("absolute seek to {seek_from:?}");
            let unclamped_idx = unclamped.seek(seek_from).expect("seek in range");
            let clamped_idx = clamped.seek(seek_from).expect("seek in range");

            assert_eq!(
                unclamped_idx, clamped_idx,
                "seeks should be identical for values in range of clamp"
            );

            assert_eq!(
                read_1(&mut unclamped).unwrap(),
                read_1(&mut clamped).unwrap(),
                "current position is in range, we should be able to read 1 byte"
            );
        }

        // Check SeekFrom::Current
        for current_idx in test_range.clone() {
            println!("current_idx: {current_idx}");
            let all_relative_seeks =
                all_possible_valid_relative_seeks(current_idx.into(), 8);
            assert_eq!(
                all_relative_seeks.clone().count(),
                8,
                "sanity: 8 possible relative seeks"
            );

            for seek_from in all_relative_seeks {
                println!("relative seek to {seek_from:?}");
                // First seek to current, just to reset last seeks
                let unclamped_idx = unclamped
                    .seek(SeekFrom::Start(current_idx.into()))
                    .expect("seek in range");
                let clamped_idx = clamped
                    .seek(SeekFrom::Start(current_idx.into()))
                    .expect("seek in range");
                assert_eq!(
                    unclamped_idx, clamped_idx,
                    "seeks should be identical for values in range of clamp"
                );

                // Now actually relative seek
                let unclamped_idx = unclamped.seek(seek_from).expect("seek in range");
                let clamped_idx = clamped.seek(seek_from).expect("seek in range");
                assert_eq!(
                    unclamped_idx, clamped_idx,
                    "seeks should be identical for values in range of clamp"
                );

                // Confirm that data can be read
                assert_eq!(
                    read_1(&mut unclamped).unwrap(),
                    read_1(&mut clamped).unwrap(),
                    "current position is in range, we should be able to read 1 byte"
                );
            }
        }
    }

    #[test]
    fn ensure_clamp_errors_on_invalid_idx() {
        // Arrange
        let data_range = 0..16u8;
        let test_data: Vec<_> = data_range.clone().collect();
        let clamp_range = ..8u64;
        let mut cursor = Cursor::new(test_data);
        let starting_pos = 2;
        // Move cursor 2 from starting position, to test nonzero self.start.
        assert_eq!(
            cursor.seek(SeekFrom::Start(starting_pos)).unwrap(),
            starting_pos,
            "sanity"
        );
        let mut clamped = ClampedSeek::new(cursor, clamp_range).unwrap();

        fn seek_read(
            mut clamped: &mut ClampedSeek<impl Read + Seek>,
            seek_from: SeekFrom,
        ) -> Result<u8> {
            let new_pos = clamped
                .seek(seek_from)
                .expect("we only call this with offsets >= 0");
            assert_eq!(
                clamped.inner.stream_position().unwrap(),
                new_pos + clamped.start,
                "returned seek position should be relative to self.start"
            );

            let result = read_1(&mut clamped);
            match result {
                Ok(val) => {
                    assert_eq!(
                        clamped.cursor,
                        new_pos + 1,
                        "success should advance cursor"
                    );
                    Ok(val)
                }
                err @ Err(_) => {
                    assert_eq!(
                        clamped.cursor, new_pos,
                        "fail should not advance cursor"
                    );
                    err
                }
            }
        }

        // Act + assert
        assert_eq!(seek_read(&mut clamped, SeekFrom::End(-2)).unwrap(), 8);
        assert_eq!(seek_read(&mut clamped, SeekFrom::End(-1)).unwrap(), 9);
        assert_eq!(
            seek_read(&mut clamped, SeekFrom::End(0))
                .unwrap_err()
                .kind(),
            ErrorKind::UnexpectedEof
        );
        assert_eq!(
            seek_read(&mut clamped, SeekFrom::End(1))
                .unwrap_err()
                .kind(),
            ErrorKind::UnexpectedEof
        );

        assert_eq!(
            clamped.seek(SeekFrom::End(-10)).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            clamped.seek(SeekFrom::End(-9)).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(seek_read(&mut clamped, SeekFrom::End(-8)).unwrap(), 2);
        assert_eq!(seek_read(&mut clamped, SeekFrom::End(-7)).unwrap(), 3);

        assert_eq!(seek_read(&mut clamped, SeekFrom::Start(0)).unwrap(), 2);
        assert_eq!(seek_read(&mut clamped, SeekFrom::Start(1)).unwrap(), 3);

        assert_eq!(seek_read(&mut clamped, SeekFrom::Start(6)).unwrap(), 8);
        assert_eq!(seek_read(&mut clamped, SeekFrom::Start(7)).unwrap(), 9);
        assert_eq!(
            seek_read(&mut clamped, SeekFrom::Start(8))
                .unwrap_err()
                .kind(),
            ErrorKind::UnexpectedEof
        );
        assert_eq!(
            seek_read(&mut clamped, SeekFrom::Start(9))
                .unwrap_err()
                .kind(),
            ErrorKind::UnexpectedEof
        );

        fn relative_seek(
            clamped: &mut ClampedSeek<impl Read + Seek>,
            current: u64,
            relative: i64,
        ) -> std::io::Result<()> {
            clamped
                .seek(SeekFrom::Start(current))
                .expect("we only call this with current offset >= 0");
            let new_pos = clamped.seek(SeekFrom::Current(relative))?;
            assert_eq!(
                clamped.inner.stream_position().unwrap(),
                new_pos + clamped.start,
                "returned seek position should be relative to self.start"
            );

            Ok(())
        }

        fn relative_seek_read(
            mut clamped: &mut ClampedSeek<impl Read + Seek>,
            current: u64,
            relative: i64,
        ) -> std::io::Result<u8> {
            relative_seek(clamped, current, relative).expect("seeks should succeed");
            let new_pos = clamped.stream_position().unwrap();
            let result = read_1(&mut clamped);
            match result {
                Ok(val) => {
                    assert_eq!(
                        clamped.cursor,
                        new_pos + 1,
                        "success should advance cursor"
                    );
                    Ok(val)
                }
                err @ Err(_) => {
                    assert_eq!(
                        clamped.cursor, new_pos,
                        "fail should not advance cursor"
                    );
                    err
                }
            }
        }

        assert_eq!(
            relative_seek(&mut clamped, 0, -2).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            relative_seek(&mut clamped, 0, -1).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(relative_seek_read(&mut clamped, 0, 0).unwrap(), 2);
        assert_eq!(relative_seek_read(&mut clamped, 0, 1).unwrap(), 3);

        assert_eq!(
            relative_seek(&mut clamped, 1, -3).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            relative_seek(&mut clamped, 1, -2).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(relative_seek_read(&mut clamped, 1, -1).unwrap(), 2);
        assert_eq!(relative_seek_read(&mut clamped, 1, 0).unwrap(), 3);
        assert_eq!(relative_seek_read(&mut clamped, 1, 1).unwrap(), 4);

        assert_eq!(relative_seek_read(&mut clamped, 6, -1).unwrap(), 7);
        assert_eq!(relative_seek_read(&mut clamped, 6, 0).unwrap(), 8);
        assert_eq!(relative_seek_read(&mut clamped, 6, 1).unwrap(), 9);
        assert_eq!(
            relative_seek_read(&mut clamped, 6, 2).unwrap_err().kind(),
            ErrorKind::UnexpectedEof
        );
        assert_eq!(
            relative_seek_read(&mut clamped, 6, 3).unwrap_err().kind(),
            ErrorKind::UnexpectedEof
        );

        assert_eq!(relative_seek_read(&mut clamped, 7, -1).unwrap(), 8);
        assert_eq!(relative_seek_read(&mut clamped, 7, 0).unwrap(), 9);
        assert_eq!(
            relative_seek_read(&mut clamped, 7, 1).unwrap_err().kind(),
            ErrorKind::UnexpectedEof
        );
        assert_eq!(
            relative_seek_read(&mut clamped, 7, 2).unwrap_err().kind(),
            ErrorKind::UnexpectedEof
        );
    }
}
