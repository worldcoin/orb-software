pub use url::ParseError as UrlParseErr;

use url::Url;

use crate::{orb_id::OrbId, Backend};

/// Access to all the urls that require parameterization on [`Backend`] and orb id.
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct Endpoints {
    pub ai_volume: Url,
    pub auth: Url,
    pub ping: Url,
}

impl Endpoints {
    /// Create a new set of URLs for the given `backend` and `orb_id`.
    ///
    /// # Errors
    /// Errors if the `orb_id` would result in an invalid URL.
    pub fn new(backend: Backend, orb_id: &OrbId) -> Self {
        let infix = match backend {
            Backend::Prod => "orb",
            Backend::Staging => "stage.orb",
            Backend::AnalysisMl => "analysis.ml",
        };

        /// Safer way to assemble URLs involving `OrbId`
        fn concat_urls(prefix: &str, orb_id: &OrbId, suffix: &str) -> Url {
            Url::parse(prefix)
                .and_then(|url| url.join(&format!("{}/", orb_id.as_str())))
                .and_then(|url| url.join(suffix))
                .expect("urls with validated orb ids should always parse")
        }

        Self {
            ai_volume: concat_urls(
                &format!("https://management.{infix}.worldcoin.org/api/v1/orbs/"),
                orb_id,
                "keys/aivolume",
            ),
            auth: Url::parse(&format!("https://auth.{infix}.worldcoin.org/api/v1/"))
                .expect("urls with validated orb ids should always parse"),
            ping: concat_urls(
                &format!("https://management.{infix}.worldcoin.org/api/v1/orbs/"),
                orb_id,
                "",
            ),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_config() {
        let stage = Endpoints::new(Backend::Staging, &"ea2ea744".parse().unwrap());
        let prod = Endpoints::new(Backend::Prod, &"ea2ea744".parse().unwrap());
        let ml = Endpoints::new(Backend::AnalysisMl, &"ea2ea744".parse().unwrap());

        assert_eq!(
            stage.ai_volume.as_str(),
            "https://management.stage.orb.worldcoin.org/api/v1/orbs/ea2ea744/keys/aivolume"
        );
        assert_eq!(
            prod.ai_volume.as_str(),
            "https://management.orb.worldcoin.org/api/v1/orbs/ea2ea744/keys/aivolume"
        );
        assert_eq!(
            ml.ai_volume.as_str(),
            "https://management.analysis.ml.worldcoin.org/api/v1/orbs/ea2ea744/keys/aivolume"
        );

        assert_eq!(
            stage.auth.as_str(),
            "https://auth.stage.orb.worldcoin.org/api/v1/"
        );
        assert_eq!(prod.auth.as_str(), "https://auth.orb.worldcoin.org/api/v1/");
        assert_eq!(
            ml.auth.as_str(),
            "https://auth.analysis.ml.worldcoin.org/api/v1/"
        );

        assert_eq!(
            stage.ping.as_str(),
            "https://management.stage.orb.worldcoin.org/api/v1/orbs/ea2ea744/"
        );
        assert_eq!(
            prod.ping.as_str(),
            "https://management.orb.worldcoin.org/api/v1/orbs/ea2ea744/"
        );
        assert_eq!(
            ml.ping.as_str(),
            "https://management.analysis.ml.worldcoin.org/api/v1/orbs/ea2ea744/"
        );
    }
}
