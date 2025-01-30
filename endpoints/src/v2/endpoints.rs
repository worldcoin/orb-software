use url::Url;

use crate::{Backend, OrbId};

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct Endpoints {
    pub status: Url,
}

impl Endpoints {
    pub fn new(backend: Backend, orb_id: &OrbId) -> Self {
        let subdomain = match backend {
            Backend::Prod => "orb",
            Backend::Staging => "stage.orb",
            Backend::Analysis => unimplemented!(),
        };

        fn concat_urls(prefix: &str, orb_id: &OrbId, suffix: &str) -> Url {
            Url::parse(prefix)
                .and_then(|url| url.join(&format!("{}/", orb_id.as_str())))
                .and_then(|url| url.join(suffix))
                .expect("urls with validated orb ids should always parse")
        }

        Self {
            status: concat_urls(
                &format!("https://management.{subdomain}.worldcoin.org/api/v2/orbs/"),
                orb_id,
                "status",
            ),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_endpoints() {
        let orb_id = "ea2ea744".parse().unwrap();
        let stage = Endpoints::new(Backend::Staging, &orb_id);
        let prod = Endpoints::new(Backend::Prod, &orb_id);

        assert_eq!(
            stage.status.as_str(),
            "https://management.stage.orb.worldcoin.org/api/v2/orbs/ea2ea744/status"
        );
        assert_eq!(
            prod.status.as_str(),
            "https://management.orb.worldcoin.org/api/v2/orbs/ea2ea744/status"
        );
    }

    #[test]
    #[should_panic(expected = "not implemented")]
    fn test_analysis_backend_unimplemented() {
        let orb_id = "ea2ea744".parse().unwrap();
        let _analysis = Endpoints::new(Backend::Analysis, &orb_id);
    }
}
