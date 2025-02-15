#![deny(unsafe_code)]

/// Concatenates const strings. Unlike [`std::concat!()`], arguments can be full
/// expressions rather than only string literals.
#[macro_export]
macro_rules! const_concat {
    // Recursive case
    ($a:expr, $b:expr, $($tail:expr),+ $(,)?) => {
        $crate::const_concat!($crate::const_concat!($a, $b), $($tail),+)
    };
    // Base case
    ($a:expr, $b:expr $(,)?) => {
        // using inner const block upgrades from temporary lifetime to static lifetime
        // outer const block forces evaluation at compile time, even when assigned
        // to a regular variable.
        const {
            const {
                $crate::concat_strs::<{$a.len() + $b.len()}>($a, $b)
            }.as_str()
        }
    };
}

/// Concatenates two strs, using a statically allocated buffer of `BUF_SIZE`.
///
/// # Panics
/// Panics if `a.len() + b.len() > MAX_LEN`
pub const fn concat_strs<const MAX_LEN: usize>(
    a: &'static str,
    b: &'static str,
) -> ArrayStr<MAX_LEN> {
    assert!(
        a.len() + b.len() <= MAX_LEN,
        "buffer is not large enough to hold the strings"
    );
    let mut buf = [0; MAX_LEN];
    copy_slice(a.as_bytes(), &mut buf, 0);
    copy_slice(b.as_bytes(), &mut buf, a.len());
    ArrayStr {
        buf,
        len: a.len() + b.len(),
    }
}

/// A string backed by a fixed-size array of length `MAX_LEN` bytes.
#[derive(Debug)]
pub struct ArrayStr<const MAX_LEN: usize> {
    buf: [u8; MAX_LEN],
    len: usize,
}
impl<const MAX_LEN: usize> ArrayStr<MAX_LEN> {
    pub const fn as_str(&self) -> &str {
        // TODO: For non-const use cases, it would be faster to do from_utf8_unchecked,
        // but that would require unsafe.
        // Most users will be using this from `const_concat!()` so its fine.
        let Ok(s) = core::str::from_utf8(self.buf.split_at(self.len).0) else {
            // Buffer will always be utf8 because it was originally utf8 strings
            unreachable!()
        };
        s
    }
}

impl<const MAX_LEN: usize> std::ops::Deref for ArrayStr<MAX_LEN> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl<T: AsRef<str>, const MAX_LEN: usize> PartialEq<T> for ArrayStr<MAX_LEN> {
    fn eq(&self, other: &T) -> bool {
        other.as_ref() == self.as_str()
    }
}

const fn copy_slice<const BUF_SIZE: usize>(
    from: &[u8],
    to: &mut [u8; BUF_SIZE],
    offset: usize,
) {
    let mut index = 0;
    while index < from.len() {
        to[offset + index] = from[index];
        index += 1;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_const_concat() {
        const FOO: &str = "foo";
        const BAR: &str = "bar";
        const BAZ: &str = "baz";

        const FOOBAR: &str = const_concat!(FOO, BAR);
        assert_eq!(FOOBAR, "foobar");

        const FOOBARBAZ: &str = const_concat!(FOO, BAR, BAZ);
        assert_eq!(FOOBARBAZ, "foobarbaz");

        const FOOBARBAZ_COMMA: &str = const_concat!(FOO, BAR, BAZ,);
        assert_eq!(FOOBARBAZ_COMMA, "foobarbaz");

        const AB_LITERAL: &str = const_concat!("a", "b");
        assert_eq!(AB_LITERAL, "ab");
    }

    #[test]
    fn test_use_outer_generics() {
        struct S;
        impl S {
            const C: &str = "associated ";
            const fn concat() -> &'static str {
                const_concat!(Self::C, "const")
            }
        }
        assert_eq!(S::concat(), "associated const");
    }

    #[test]
    fn test_larger_buf_still_works() {
        assert_eq!(concat_strs::<10>("a", "aa"), "aaa");
    }

    #[test]
    #[should_panic]
    fn test_smaller_buf_fails() {
        concat_strs::<2>("a", "aa");
    }

    #[test]
    fn test_exact_size_buf_works() {
        assert_eq!(concat_strs::<3>("a", "aa"), "aaa");
    }
}
