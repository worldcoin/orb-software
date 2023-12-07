use std::{env::VarError, str::FromStr};

pub const ORB_BACKEND_ENV_VAR_NAME: &str = "ORB_BACKEND";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    Prod,
    Staging,
}

impl Default for Backend {
    /// # Panics
    /// Panics if the backend could not be parsed from [`ORB_BACKEND_ENV_VAR_NAME`].
    fn default() -> Self {
        Self::from_env().expect("could not parse `Backend` from env")
    }
}

impl Backend {
    /// Choose the backend based on the environment variable.
    pub fn from_env() -> Result<Self, BackendFromEnvError> {
        let v = std::env::var(ORB_BACKEND_ENV_VAR_NAME).map_err(|e| match e {
            VarError::NotPresent => BackendFromEnvError::NotSet,
            VarError::NotUnicode(_) => BackendFromEnvError::Invalid(BackendParseErr),
        })?;

        Self::from_str(&v).map_err(|e| e.into())
    }
}

impl FromStr for Backend {
    type Err = BackendParseErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "prod" | "production" => Ok(Self::Prod),
            "stage" | "staging" | "dev" | "development" => Ok(Self::Staging),
            _ => Err(BackendParseErr),
        }
    }
}

// ---- Error types ----

/// Error from parsing a string into [`crate::Backend`].
#[derive(Debug, Eq, PartialEq)]
pub struct BackendParseErr;

impl std::fmt::Display for BackendParseErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to parse `Backend` from str")
    }
}

impl std::error::Error for BackendParseErr {}

/// Error from parsing env var into [`crate::Backend`].
#[derive(Debug, Eq, PartialEq)]
pub enum BackendFromEnvError {
    NotSet,
    Invalid(BackendParseErr),
}

impl std::fmt::Display for BackendFromEnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendFromEnvError::NotSet => {
                write!(f, "env var {ORB_BACKEND_ENV_VAR_NAME} was not set")
            }
            BackendFromEnvError::Invalid(_e) => {
                write!(f, "env var {ORB_BACKEND_ENV_VAR_NAME} failed to parse")
            }
        }
    }
}

impl std::error::Error for BackendFromEnvError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BackendFromEnvError::NotSet => None,
            BackendFromEnvError::Invalid(e) => Some(e),
        }
    }
}

impl From<BackendParseErr> for BackendFromEnvError {
    fn from(value: BackendParseErr) -> Self {
        Self::Invalid(value)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_backend_parse() {
        assert_eq!(Backend::from_str("prod").unwrap(), Backend::Prod);
        assert_eq!(Backend::from_str("pRod").unwrap(), Backend::Prod);
        assert_eq!(Backend::from_str("stage").unwrap(), Backend::Staging);
        assert_eq!(Backend::from_str("staGe").unwrap(), Backend::Staging);
        assert_eq!(Backend::from_str("dev").unwrap(), Backend::Staging);
        assert_eq!(Backend::from_str("foobar"), Err(BackendParseErr));
    }
}
