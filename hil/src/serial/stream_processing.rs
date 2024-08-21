//! Utilities for processing streams of bytes.

use std::collections::VecDeque;

use futures::{TryStream, TryStreamExt as _};

use crate::serial::LOGIN_PROMPT;

use super::KERNEL_PANIC_PATERN;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum SerialLogEvent {
    /// Login prompt was detected
    LoginPrompt,
    /// Kernel panic was detected
    KernelPanic,
}

pub struct SerialProcessor {
    login_prompt_pattern: Vec<u8>,
    kernel_panic_pattern: Vec<u8>,
}

impl SerialProcessor {
    pub fn new() -> Self {
        Self {
            login_prompt_pattern: LOGIN_PROMPT.to_owned().into_bytes(),
            kernel_panic_pattern: KERNEL_PANIC_PATERN.to_owned().into_bytes(),
        }
    }

    pub fn listen_for_events<B, E>(
        self,
        byte_stream: impl TryStream<Ok = B, Error = E>,
    ) -> impl TryStream<Ok = SerialLogEvent, Error = E>
    where
        B: AsRef<[u8]>,
    {
        let mut login_prompt_detector =
            make_streamed_buf_comparison(self.login_prompt_pattern);
        let mut kernel_panic_detector =
            make_streamed_buf_comparison(self.kernel_panic_pattern);

        byte_stream
            .map_ok(move |bytes| {
                let num_login_prompts = login_prompt_detector(bytes.as_ref());
                let num_kernel_panics = kernel_panic_detector(bytes.as_ref());
                let login_iter =
                    (0..num_login_prompts).map(|_| Ok(SerialLogEvent::LoginPrompt));
                let panic_iter =
                    (0..num_kernel_panics).map(|_| Ok(SerialLogEvent::KernelPanic));
                futures::stream::iter(login_iter.chain(panic_iter))
            })
            .try_flatten()
    }
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
fn make_streamed_buf_comparison<'p>(
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
    use std::convert::Infallible;

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
        let text = haiku();
        let expected_num_matches = text.matches(&pat).count();
        println!("expecting {expected_num_matches} matches");
        let text = text.into_bytes();

        for chunk_size in 1..text.len() {
            println!("chunk_size: {chunk_size}");
            let text_it = text.chunks(chunk_size);
            let stream = futures::stream::iter(text_it.map(Ok::<_, Infallible>));
            let mut output_stream = {
                let mut processor = SerialProcessor::new();
                processor.login_prompt_pattern = pat.clone().into_bytes();
                processor.listen_for_events(stream)
            };

            for _ in 0..expected_num_matches {
                assert_eq!(
                    output_stream.try_next().await.expect("infallible"),
                    Some(SerialLogEvent::LoginPrompt),
                    "expected to encounter prompt events"
                );
                println!("found match");
            }
            assert_eq!(
                output_stream.try_next().await.expect("infallible"),
                None,
                "after original text body is exhausted, expected stream to terminate"
            );
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
