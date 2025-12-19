//! Code related to [`Key`]

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum InvalidKeyErr {
    #[error("encountered empty string which is not allowed")]
    EmptyString,
    #[error("encountered non-ascii character")]
    NotAscii,
    #[error("encountered non-lowercase character")]
    NotLowercase,
    #[error("only alphanumeric, hyphen, and underscore are allowed as characters")]
    DisallowedCharacter,
}

/// A key that has been validated to meet the necessary characterset.
///
/// All operations on [`crate::Client`] use `Key` to address the contents.
/// The benefits of using a `Key` directly instead of a `&str` is that the validation can be done
/// up front instead of on every function call to `Client`.
///
/// See [`InvalidKeyErr`] for an explanation of the necessary criteria for a key.
#[derive(Debug, Eq, PartialEq, Clone, Copy, Ord, PartialOrd, Hash)]
pub struct Key<'a>(&'a str);

impl<'a> AsRef<str> for Key<'a> {
    fn as_ref(&self) -> &str {
        self.0
    }
}

/// Basically the same as `TryInto<Key<'a>, Error=InvalidKey>`.
pub trait TryIntoKey<'a>: private::Sealed {
    fn to_key(self) -> Result<Key<'a>, InvalidKeyErr>;
}

mod private {
    pub trait Sealed {}
}

impl<'a, T: TryInto<Key<'a>, Error = InvalidKeyErr>> TryIntoKey<'a> for T {
    fn to_key(self) -> Result<Key<'a>, InvalidKeyErr> {
        self.try_into()
    }
}

impl<'a> TryIntoKey<'a> for Key<'a> {
    fn to_key(self) -> Result<Key<'a>, InvalidKeyErr> {
        Ok(self)
    }
}

impl<'a> private::Sealed for Key<'a> {}

impl<'a, T: TryInto<Key<'a>, Error = InvalidKeyErr>> private::Sealed for T {}

impl<'a> TryFrom<&'a str> for Key<'a> {
    type Error = InvalidKeyErr;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        validate_key(value)
    }
}

impl<'a> TryFrom<&'a String> for Key<'a> {
    type Error = InvalidKeyErr;

    fn try_from(value: &'a String) -> std::result::Result<Self, Self::Error> {
        validate_key(value)
    }
}

impl<'a> TryFrom<&'a [u8]> for Key<'a> {
    type Error = InvalidKeyErr;

    fn try_from(value: &'a [u8]) -> std::result::Result<Self, Self::Error> {
        let s = str::from_utf8(value).map_err(|_| InvalidKeyErr::NotAscii)?;
        validate_key(s)
    }
}

fn validate_key(s: &str) -> Result<Key<'_>, InvalidKeyErr> {
    if s.is_empty() {
        return Err(InvalidKeyErr::EmptyString);
    }
    if !s.is_ascii() {
        return Err(InvalidKeyErr::NotAscii);
    }
    for c in s.bytes() {
        if c.is_ascii_uppercase() {
            return Err(InvalidKeyErr::NotLowercase);
        }
        if !c.is_ascii_alphanumeric() && ![b'-', b'_'].contains(&c) {
            return Err(InvalidKeyErr::DisallowedCharacter);
        }
    }

    Ok(Key(s))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn valid_examples_should_parse() {
        let examples = ["yippee", "y1pp33", "31337"];
        for e in examples {
            assert_eq!(e.to_key(), Ok(Key(e)));
        }
    }

    #[test]
    fn empty_string_errors() {
        assert_eq!("".to_key(), Err(InvalidKeyErr::EmptyString))
    }

    #[test]
    fn non_ascii_should_error() {
        let examples = ["🙀", "ඞ", "légume", "🅱️ased"];
        for e in examples {
            assert_eq!(e.to_key(), Err(InvalidKeyErr::NotAscii));
        }
    }

    #[test]
    fn ascii_control_codes_should_be_rejected() {
        let examples = ["fo\nd", "fi\re", "fo\0m", &(0x03 as char).to_string()];
        for e in examples {
            assert_eq!(e.to_key(), Err(InvalidKeyErr::DisallowedCharacter));
        }
    }

    #[test]
    fn uppercase_should_be_rejected() {
        let examples = ["B", "UwU", "oWo"];
        for e in examples {
            assert_eq!(e.to_key(), Err(InvalidKeyErr::NotLowercase));
        }
    }

    #[test]
    fn non_alphanumeric_should_be_rejected() {
        let examples = ["foo bar", " hi", "bye ", " ", "ru$t"];
        for e in examples {
            assert_eq!(e.to_key(), Err(InvalidKeyErr::DisallowedCharacter));
        }
    }
}
