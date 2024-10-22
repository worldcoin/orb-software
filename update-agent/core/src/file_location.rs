use std::{borrow::Cow, path::PathBuf, str::FromStr};

use url::{ParseError, Url};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error(
        "Failed to extract file path from `file://` scheme URL; URL provided: `{0}`"
    )]
    ExtractFilePath(Url),
    #[error("HTTP protocol is not supported, only HTTPS; URL provided: `{0}`")]
    Http(Url),
    #[error("Failed parsing `{url}` as URL")]
    Parse { url: String, source: ParseError },
    #[error("Scheme `{}` is not supported; URL provided: `{0}`", .0.scheme())]
    Scheme(Url),
}

impl Error {
    fn extract_file_path(url: Url) -> Self {
        Self::ExtractFilePath(url)
    }

    fn http(url: Url) -> Self {
        Self::Http(url)
    }

    fn parse<'a, T: Into<Cow<'a, str>>>(url: T, source: ParseError) -> Self {
        Self::Parse {
            url: url.into().to_string(),
            source,
        }
    }

    fn scheme(url: Url) -> Self {
        Self::Scheme(url)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalOrRemote {
    Local(PathBuf),
    Remote(Url),
}

impl LocalOrRemote {
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(..))
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote(..))
    }

    pub fn parse(s: &str) -> Result<Self, Error> {
        let location = match Url::parse(s) {
            Ok(url) if url.scheme() == "https" => url.into(),
            Ok(url) if url.scheme() == "file" => url
                .to_file_path()
                .map_err(|()| Error::extract_file_path(url))?
                .into(),
            Ok(url) if url.scheme() == "http" => return Err(Error::http(url)),
            Ok(url) => return Err(Error::scheme(url)),
            Err(ParseError::RelativeUrlWithoutBase) => {
                PathBuf::from(s.to_string()).into()
            }
            Err(e) => return Err(Error::parse(s, e)),
        };
        Ok(location)
    }
}

impl From<PathBuf> for LocalOrRemote {
    fn from(path: PathBuf) -> Self {
        Self::Local(path)
    }
}

impl From<Url> for LocalOrRemote {
    fn from(url: Url) -> Self {
        Self::Remote(url)
    }
}

impl FromStr for LocalOrRemote {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

mod serde_impl {
    use std::str::FromStr;

    use serde::{
        de, ser::Error as _, Deserialize, Deserializer, Serialize, Serializer,
    };

    use super::LocalOrRemote;

    impl<'de> Deserialize<'de> for LocalOrRemote {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            FromStr::from_str(&s).map_err(de::Error::custom)
        }
    }

    impl Serialize for LocalOrRemote {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                Self::Remote(url) => url.serialize(serializer),
                Self::Local(path) => match path.to_str() {
                    Some(s) => format!("file://{s}").serialize(serializer),
                    None => Err(S::Error::custom(
                        "local path contains unvalid UTF-8 characters",
                    )),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{Error, LocalOrRemote};

    #[test]
    fn https_url_is_remote() -> Result<(), Box<dyn std::error::Error>> {
        let url = "https://foo.bar?baz=True";
        let expected = LocalOrRemote::Remote(Url::parse(url)?);
        let actual = LocalOrRemote::parse(url)?;
        assert_eq!(expected, actual);
        Ok(())
    }

    #[test]
    fn relative_path_is_local() -> Result<(), Box<dyn std::error::Error>> {
        let url = "/foo/bar/baz";
        let expected = LocalOrRemote::Local(url.into());
        let actual = LocalOrRemote::parse(url)?;
        assert_eq!(expected, actual);
        Ok(())
    }

    #[test]
    fn file_url_is_local() -> Result<(), Box<dyn std::error::Error>> {
        let url = "file:///foo/bar/baz";
        let expected = LocalOrRemote::Local("/foo/bar/baz".into());
        let actual = LocalOrRemote::parse(url)?;
        assert_eq!(expected, actual);
        Ok(())
    }

    #[test]
    fn http_remote_is_not_supported() -> Result<(), Box<dyn std::error::Error>> {
        let url = "http://foo.bar?baz=True";
        let expected = Err(Error::http(Url::parse(url)?));
        let actual = LocalOrRemote::parse(url);
        assert_eq!(expected, actual);
        Ok(())
    }

    #[test]
    fn other_schema_is_not_supported() -> Result<(), Box<dyn std::error::Error>> {
        let url = "other://foo.bar?baz=True";
        let expected = Err(Error::scheme(Url::parse(url)?));
        let actual = LocalOrRemote::parse(url);
        assert_eq!(expected, actual);
        Ok(())
    }
}
