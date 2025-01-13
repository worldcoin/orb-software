use std::env;

use eyre::{self, bail};

const ORB_BACKEND_ENV_VAR_NAME: &str = "ORB_BACKEND";

pub struct Config {
    pub auth_url: url::Url,
    pub ping_url: url::Url,
}

impl Config {
    /// Create a new config for the given `backend` and `orb_id`.
    ///
    /// # Panics
    ///  - If failed to parse the `auth_url` or `ping_url`
    #[must_use]
    pub fn new(backend: Backend, orb_id: &str) -> Self {
        let (auth, ping) = match backend {
            Backend::Prod => ("auth.orb", "management.orb"),
            Backend::Staging => ("auth.stage.orb", "management.stage.orb"),
            Backend::Analysis => ("auth.analysis.orb", "management.analysis.orb"),
        };
        Config {
            auth_url: url::Url::parse(&format!("https://{auth}.worldcoin.org/api/v1/"))
                .unwrap(),
            ping_url: url::Url::parse(&format!(
                "https://{ping}.worldcoin.org/api/v1/orbs/{orb_id}"
            ))
            .unwrap(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Prod,
    Staging,
    Analysis,
}

const DEFAULT_BACKEND: Backend = Backend::Prod;

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend {
    /// Create a new backend config instance
    #[must_use]
    pub fn new() -> Self {
        Self::from_env().unwrap_or(DEFAULT_BACKEND)
    }

    /// Choose the backend based on the `ORB_BACKEND` environment variable.
    fn from_env() -> eyre::Result<Self> {
        let v = env::var(ORB_BACKEND_ENV_VAR_NAME)?;
        match v.trim().to_lowercase().as_str() {
            "prod" => Ok(Backend::Prod),
            "stage" | "dev" => Ok(Backend::Staging),
            "analysis" | "analysis.ml" | "analysis-ml" => Ok(Self::Analysis),
            _ => {
                bail!("unknown value for backend");
            }
        }
    }
}

#[cfg(test)]
mod test {
    use serial_test::serial;

    #[test]
    #[serial]
    fn default_backend() {
        assert_eq!(super::Backend::new(), super::DEFAULT_BACKEND);
        assert_eq!(super::Backend::default(), super::DEFAULT_BACKEND);
    }

    #[test]
    #[serial]
    fn custom_backend() {
        std::env::set_var(super::ORB_BACKEND_ENV_VAR_NAME, "prod");
        assert_eq!(super::Backend::new(), super::Backend::Prod);
        std::env::set_var(super::ORB_BACKEND_ENV_VAR_NAME, "stage");
        assert_eq!(super::Backend::new(), super::Backend::Staging);
        std::env::set_var(super::ORB_BACKEND_ENV_VAR_NAME, "dev");
        assert_eq!(super::Backend::new(), super::Backend::Staging);
        std::env::set_var(super::ORB_BACKEND_ENV_VAR_NAME, "SOME RANDOM STRING");
        assert_eq!(super::Backend::new(), super::DEFAULT_BACKEND);
        std::env::remove_var(super::ORB_BACKEND_ENV_VAR_NAME);
    }
}
