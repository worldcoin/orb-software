//! Utilities for processing streams of bytes.

use std::collections::VecDeque;

use futures::{Stream, StreamExt as _, TryStream, TryStreamExt as _};

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum SerialLogEvent {
    LoginPrompt,
}

/// Attaches to a stream of bytes and calls `callback` every time `pattern` is
/// encountered.
pub fn listen_for_pattern<'p, B, E>(
    pattern: impl AsRef<[u8]> + 'p,
    byte_stream: impl TryStream<Ok = B, Error = E> + 'p,
) -> impl TryStream<Ok = SerialLogEvent, Error = E> + 'p
where
    B: AsRef<[u8]>,
{
    let mut compare_func = make_streamed_buf_comparison(pattern);
    byte_stream.map_ok(move |bytes| {
        let num_detected = compare_func(bytes.as_ref());
        // TODO: find or make a combinator that allows me to reuse a buffer.
        SerialLogEvent::LoginPrompt
    })
}

/// Creates a closure used to match a stream of bytes against `pattern`.
///
/// The closure returns how many times `pattern` was found since the last time
/// the closure returned a nonzero number.
///
/// # Example
/// // TODO: write example where you add up all the return vals of the closure
///
/// # Panics
/// Panics if `pattern.len() == 0`.
pub fn make_streamed_buf_comparison<'p>(
    pattern: impl AsRef<[u8]> + 'p,
) -> impl FnMut(&[u8]) -> u8 + 'p {
    assert!(
        !pattern.as_ref().is_empty(),
        "pattern must be nonzero in length"
    );

    let mut buf = VecDeque::<u8>::with_capacity(pattern.as_ref().len());
    let mut num_bytes_matched = 0;
    move |bytes: &[u8]| {
        let pattern = pattern.as_ref();
        debug_assert!(
            num_bytes_matched < pattern.len(),
            "sanity check: cannot match >= the number of bytes in `expected`, \
            because we always reset to zero when we hit a match."
        );
        buf.extend(bytes);

        let mut num_full_matches = 0;
        while num_bytes_matched < buf.len() {
            if buf[num_bytes_matched] == pattern[num_bytes_matched] {
                num_bytes_matched += 1;
                if num_bytes_matched == pattern.len() {
                    num_bytes_matched = 0;
                    num_full_matches += 1;
                    buf = buf.split_off(pattern.len());
                }
            } else {
                buf.pop_front();
                num_bytes_matched = 0;
            }
        }

        num_full_matches
    }
}

#[cfg(test)]
mod test_listen {
    use super::*;

    fn haiku() -> String {
        String::from(
            "\
            yellow is sussy! \
            I know their twerk goes crazy \
            but I saw them vent. \
        ",
        )
    }

    #[tokio::test]
    async fn test_listen() {
        let pat = String::from("I ");
        let text = haiku().into_bytes();

        for chunk_size in 1..text.len() {
            println!("chunk_size: {chunk_size}");
            let mut num_times_found = 0;
            let text_it = text.chunks(chunk_size);
            let stream = futures::stream::iter(text_it.clone());
            let mut output_stream =
                listen_for_pattern(pat.as_bytes(), stream, || num_times_found += 1);

            for chunk in text_it {
                assert_eq!(
                    output_stream.next().await,
                    Some(chunk),
                    "expected chunks identical to original text body"
                );
            }
            assert_eq!(
                output_stream.next().await,
                None,
                "after original text body is exhausted, expected stream to terminate"
            );

            drop(output_stream);
            assert_eq!(num_times_found, 2);
        }
    }
}

#[cfg(test)]
mod test_buf_comparison {
    use super::*;

    #[test]
    #[should_panic]
    fn test_empty_pattern_panics() {
        let _matcher = make_streamed_buf_comparison(b"");
    }

    #[test]
    fn test_empty_chunk() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        assert_eq!((matcher(b"")), 0);
        assert_eq!((matcher(b"patter")), 0);
        assert_eq!((matcher(b"")), 0);
        assert_eq!(matcher(b"n"), 1);
        assert_eq!((matcher(b"")), 0);
    }

    #[test]
    fn test_long_chunk() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        let long_chunk = vec![b'a', 32];
        assert_eq!((matcher(long_chunk.as_slice())), 0);
        assert_eq!(matcher(b"pattern"), 1);
    }

    #[test]
    fn test_exact_match() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        assert_eq!(matcher(b"pattern"), 1);
    }

    #[test]
    fn test_no_match() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        assert_eq!((matcher(b"nope")), 0);
    }

    #[test]
    fn test_partial_match() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        assert_eq!((matcher(b"pat")), 0);
        assert_eq!(matcher(b"tern"), 1);

        assert_eq!((matcher(b"foopat")), 0);
        assert_eq!(matcher(b"tern"), 1);

        assert_eq!((matcher(b"foopat")), 0);
        assert_eq!(matcher(b"ternbar"), 1);

        assert_eq!((matcher(b"pat")), 0);
        assert_eq!(matcher(b"ternbar"), 1);
    }

    #[test]
    fn test_overlapping_match() {
        let mut matcher = make_streamed_buf_comparison(b"abab");
        assert_eq!((matcher(b"aba")), 0);
        assert_eq!(matcher(b"b"), 1);
        // Prior state should be cleared, so this shouldn't match.
        assert_eq!((matcher(b"aba")), 0);
        assert_eq!(matcher(b"b"), 1);
    }

    #[test]
    fn test_multiple_matches() {
        let mut matcher = make_streamed_buf_comparison(b"ab");
        assert_eq!(matcher(b"ab"), 1);
        assert_eq!(matcher(b"cab"), 1);
        assert_eq!(matcher(b"abab"), 2);
        assert_eq!(matcher(b"ab"), 1);
    }

    #[test]
    fn test_long_pattern_short_chunks() {
        let mut matcher = make_streamed_buf_comparison(b"abcdefgh");
        assert_eq!((matcher(b"a")), 0);
        assert_eq!((matcher(b"b")), 0);
        assert_eq!((matcher(b"c")), 0);
        assert_eq!((matcher(b"d")), 0);
        assert_eq!((matcher(b"e")), 0);
        assert_eq!((matcher(b"f")), 0);
        assert_eq!((matcher(b"g")), 0);
        assert_eq!(matcher(b"h"), 1);
        assert_eq!((matcher(b"i")), 0);
    }

    #[test]
    fn test_owned_pattern() {
        let mut matcher = make_streamed_buf_comparison(Vec::from(b"pattern"));
        assert_eq!(matcher(b"pattern"), 1);
    }
}
