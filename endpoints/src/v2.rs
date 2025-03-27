use url::Url;

use crate::{concat_urls, Backend, OrbId};

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
            Backend::Local => todo!(),
        };

        Self {
            status: concat_urls(
                &format!("https://fleet.{subdomain}.worldcoin.org/api/v2/orbs/"),
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
            "https://fleet.stage.orb.worldcoin.org/api/v2/orbs/ea2ea744/status"
        );
        assert_eq!(
            prod.status.as_str(),
            "https://fleet.orb.worldcoin.org/api/v2/orbs/ea2ea744/status"
        );
    }

    #[test]
    #[should_panic(expected = "not implemented")]
    fn test_analysis_backend_unimplemented() {
        let orb_id = "ea2ea744".parse().unwrap();
        let _analysis = Endpoints::new(Backend::Analysis, &orb_id);
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn test_local_backend_unimplemented() {
        let orb_id = "ea2ea744".parse().unwrap();
        let _local = Endpoints::new(Backend::Local, &orb_id);
    }
}
