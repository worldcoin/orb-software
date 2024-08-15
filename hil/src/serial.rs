use futures::{Stream, StreamExt};
use std::collections::VecDeque;

/// Attaches to a stream of bytes and calls `callback` when encountering the
/// login prompt.
fn listen_for_pattern<'p, B>(
    pattern: &'p [u8],
    byte_stream: impl Stream<Item = B> + 'p,
    mut callback: impl FnMut() + 'p,
) -> impl Stream<Item = B> + 'p
where
    B: AsRef<[u8]>,
{
    let mut compare_func = make_streamed_buf_comparison(pattern);
    byte_stream.inspect(move |bytes| {
        for _ in 0..compare_func(bytes.as_ref()) {
            callback()
        }
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
fn make_streamed_buf_comparison(pattern: &[u8]) -> impl FnMut(&[u8]) -> u8 + '_ {
    assert!(pattern.len() > 0, "pattern must be nonzero in length");

    let mut buf = VecDeque::<u8>::with_capacity(pattern.len());
    let mut num_bytes_matched = 0;
    move |bytes: &[u8]| {
        debug_assert!(
            num_bytes_matched < pattern.len(),
            "sanity check: cannot match >= the number of bytes in `expected`, \
            because we always reset to zero when we hit a match."
        );
        buf.extend(bytes);
        while num_bytes_matched < buf.len() {
            if buf[num_bytes_matched] == pattern[num_bytes_matched] {
                num_bytes_matched += 1;
                if num_bytes_matched == pattern.len() {
                    num_bytes_matched = 0;
                    buf = buf.split_off(pattern.len());
                    return 1;
                }
            } else {
                buf.pop_front();
                num_bytes_matched = 0;
            }
        }

        0
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

    // TODO: add test that it actually counts the number of matches when the input
    // contains multiple patterns.

    #[test]
    #[should_panic]
    fn test_empty_pattern_panics() {
        let _matcher = make_streamed_buf_comparison(b"");
    }

    #[test]
    fn test_empty_chunk() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        let mut matcher = |chunk| matcher(chunk) >= 1;
        assert!(!matcher(b""));
        assert!(!matcher(b"patter"));
        assert!(!matcher(b""));
        assert!(matcher(b"n"));
        assert!(!matcher(b""));
    }

    #[test]
    fn test_long_chunk() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        let mut matcher = |chunk| matcher(chunk) >= 1;
        let long_chunk = vec![b'a', 32];
        assert!(!matcher(long_chunk.as_slice()));
        assert!(matcher(b"pattern"));
    }

    #[test]
    fn test_exact_match() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        let mut matcher = |chunk| matcher(chunk) >= 1;
        assert!(matcher(b"pattern"));
    }

    #[test]
    fn test_no_match() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        let mut matcher = |chunk| matcher(chunk) >= 1;
        assert!(!matcher(b"nope"));
    }

    #[test]
    fn test_partial_match() {
        let mut matcher = make_streamed_buf_comparison(b"pattern");
        let mut matcher = |chunk| matcher(chunk) >= 1;
        assert!(!matcher(b"pat"));
        assert!(matcher(b"tern"));

        assert!(!matcher(b"foopat"));
        assert!(matcher(b"tern"));

        assert!(!matcher(b"foopat"));
        assert!(matcher(b"ternbar"));

        assert!(!matcher(b"pat"));
        assert!(matcher(b"ternbar"));
    }

    #[test]
    fn test_overlapping_match() {
        let mut matcher = make_streamed_buf_comparison(b"abab");
        let mut matcher = |chunk| matcher(chunk) >= 1;
        assert!(!matcher(b"aba"));
        assert!(matcher(b"b"));
        // Prior state should be cleared, so this shouldn't match.
        assert!(!matcher(b"aba"));
        assert!(matcher(b"b"));
    }

    #[test]
    fn test_multiple_matches() {
        let mut matcher = make_streamed_buf_comparison(b"ab");
        let mut matcher = |chunk| matcher(chunk) >= 1;
        assert!(matcher(b"ab"));
        assert!(matcher(b"cab"));
        assert!(matcher(b"abab"));
    }

    #[test]
    fn test_long_pattern_short_chunks() {
        let mut matcher = make_streamed_buf_comparison(b"abcdefgh");
        let mut matcher = |chunk| matcher(chunk) >= 1;
        assert!(!matcher(b"a"));
        assert!(!matcher(b"b"));
        assert!(!matcher(b"c"));
        assert!(!matcher(b"d"));
        assert!(!matcher(b"e"));
        assert!(!matcher(b"f"));
        assert!(!matcher(b"g"));
        assert!(matcher(b"h"));
        assert!(!matcher(b"i"));
    }
}
