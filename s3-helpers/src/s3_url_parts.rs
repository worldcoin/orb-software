use std::{fmt::Display, str::FromStr};

use color_eyre::eyre::OptionExt as _;

/// A parsed s3 uri
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct S3Uri {
    pub bucket: String,
    pub key: String,
}

impl S3Uri {
    pub fn is_dir(&self) -> bool {
        self.key.ends_with('/') || self.key.is_empty()
    }
}

impl FromStr for S3Uri {
    type Err = color_eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (bucket, key) = s
            .strip_prefix("s3://")
            .ok_or_eyre("must be a url that starts with `s3://`")?
            .split_once('/')
            .ok_or_eyre("expected s3://<bucket>/<key>")?;
        Ok(Self {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
        })
    }
}

impl Display for S3Uri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s3://{}/{}", self.bucket, self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_prefix_ends_in_slash() {
        let url = "s3://my-bucket/the/prefix/";
        let parts = S3Uri::from_str(url).unwrap();
        assert_eq!(
            parts,
            S3Uri {
                bucket: "my-bucket".to_string(),
                key: "the/prefix/".to_string(),
            }
        );
        assert!(parts.is_dir());
        assert_eq!(url, parts.to_string())
    }

    #[test]
    fn test_valid_simple_s3_url() {
        let url = "s3://my-bucket/my-key";
        let parts = S3Uri::from_str(url).unwrap();
        assert_eq!(
            parts,
            S3Uri {
                bucket: "my-bucket".to_string(),
                key: "my-key".to_string(),
            }
        );
        assert!(!parts.is_dir());
        assert_eq!(url, parts.to_string())
    }

    #[test]
    fn test_valid_s3_url_with_complex_key() {
        let url = "s3://my-bucket/path/to/my/object.json";
        let parts = S3Uri::from_str(url).unwrap();
        assert_eq!(
            parts,
            S3Uri {
                bucket: "my-bucket".to_string(),
                key: "path/to/my/object.json".to_string(),
            }
        );
        assert!(!parts.is_dir());
        assert_eq!(url, parts.to_string())
    }

    #[test]
    fn test_valid_s3_url_with_special_chars() {
        let url = "s3://my-bucket-123/my_key-123.txt";
        let parts = S3Uri::from_str(url).unwrap();
        assert_eq!(
            parts,
            S3Uri {
                bucket: "my-bucket-123".to_string(),
                key: "my_key-123.txt".to_string(),
            }
        );
        assert!(!parts.is_dir());
        assert_eq!(url, parts.to_string())
    }

    #[test]
    fn test_empty_key() {
        let url = "s3://my-bucket/";
        let parts = S3Uri::from_str(url).unwrap();
        assert_eq!(
            parts,
            S3Uri {
                bucket: "my-bucket".to_string(),
                key: "".to_string(),
            }
        );
        assert!(parts.is_dir());
        assert_eq!(url, parts.to_string())
    }

    #[test]
    fn test_missing_s3_prefix() {
        let url = "my-bucket/my-key";
        let result = S3Uri::from_str(url);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_slash_after_bucket() {
        let url = "s3://my-bucket";
        let result = S3Uri::from_str(url);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_string() {
        let url = "";
        let result = S3Uri::from_str(url);
        assert!(result.is_err());
    }

    #[test]
    fn test_only_s3_prefix() {
        let url = "s3://";
        let result = S3Uri::from_str(url);
        assert!(result.is_err());
    }
}
