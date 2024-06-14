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
        // outer const block forces evaluation at compile time, even when assigned
        // to a regular variable.
        const {
            let Ok(s) = ::core::str::from_utf8(
                // using const block prevents dangling reference
                &const { $crate::concat_strs($a, $b, [0; $a.len() + $b.len()]) },
            ) else {
                panic!("not utf8");
            };
            s
        }
    };
}

#[doc(hidden)]
pub const fn concat_strs<const BUF_SIZE: usize>(
    a: &'static str,
    b: &'static str,
    buf: [u8; BUF_SIZE],
) -> [u8; BUF_SIZE] {
    assert!(a.len() + b.len() == BUF_SIZE);
    let buf = copy_slice(a.as_bytes(), buf, 0);
    copy_slice(b.as_bytes(), buf, a.len())
}

#[doc(hidden)]
pub const fn copy_slice<const BUF_SIZE: usize>(
    from: &[u8],
    mut to: [u8; BUF_SIZE],
    offset: usize,
) -> [u8; BUF_SIZE] {
    let mut index = 0;
    while index < from.len() {
        to[offset + index] = from[index];
        index += 1;
    }
    to
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
    }
}
