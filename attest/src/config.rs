use std::str::FromStr;
use eyre::{self, bail};
use orb_endpoints::{Backend, v1};
use orb_info::OrbId;

pub struct Config {
    pub auth_url: url::Url,
    pub ping_url: url::Url,
}

impl Config {
    /// Create a new config for the given `backend` and `orb_id`.
    ///
    /// # Panics
    ///  - If failed to parse the `orb_id`
    #[must_use]
    pub fn new(backend: Backend, orb_id: &str) -> Self {
        // Parse the orb_id string into an OrbId
        let orb_id = OrbId::from_str(orb_id)
            .expect("Invalid orb_id format");

        let endpoints = v1::Endpoints::new(backend, &orb_id);

        Config {
            auth_url: endpoints.auth,
            ping_url: endpoints.ping,
        }
    }
}

const DEFAULT_BACKEND: Backend = Backend::Prod;

#[must_use]
pub fn default_backend() -> Backend {
    get_backend().unwrap_or(DEFAULT_BACKEND)
}

fn get_backend() -> eyre::Result<Backend> {
    match orb_endpoints::Backend::from_env() {
        Ok(backend) => Ok(backend),
        Err(orb_endpoints::backend::BackendFromEnvError::NotSet) => {
            // Default to prod if not set
            Ok(DEFAULT_BACKEND)
        },
        Err(orb_endpoints::backend::BackendFromEnvError::Invalid(_)) => {
            bail!("unknown value for backend");
        }
    }
}

#[cfg(test)]
mod test {
    use serial_test::serial;
    use orb_endpoints::Backend;

    #[test]
    #[serial]
    fn default_backend() {
        assert_eq!(super::default_backend(), super::DEFAULT_BACKEND);
    }

    #[test]
    #[serial]
    fn custom_backend() {
        std::env::set_var(orb_endpoints::backend::ORB_BACKEND_ENV_VAR_NAME, "prod");
        assert_eq!(super::default_backend(), Backend::Prod);
        std::env::set_var(orb_endpoints::backend::ORB_BACKEND_ENV_VAR_NAME, "stage");
        assert_eq!(super::default_backend(), Backend::Staging);
        std::env::set_var(orb_endpoints::backend::ORB_BACKEND_ENV_VAR_NAME, "dev");
        assert_eq!(super::default_backend(), Backend::Staging);
        std::env::set_var(orb_endpoints::backend::ORB_BACKEND_ENV_VAR_NAME, "analysis");
        assert_eq!(super::default_backend(), Backend::Analysis);
        std::env::set_var(orb_endpoints::backend::ORB_BACKEND_ENV_VAR_NAME, "SOME RANDOM STRING");
        assert_eq!(super::default_backend(), super::DEFAULT_BACKEND);
        std::env::remove_var(orb_endpoints::backend::ORB_BACKEND_ENV_VAR_NAME);
    }
}
