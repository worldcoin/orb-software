use url::Url;

use crate::{concat_urls, Backend, OrbId};

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct Endpoints {
    pub status: Url,
    pub keys_challenge: Url,
    pub keys_proof: Url,
}

impl Endpoints {
    pub fn new(backend: Backend, orb_id: &OrbId) -> Self {
        let subdomain = match backend {
            Backend::Prod => "orb",
            Backend::Staging => "stage.orb",
            // legacy analysis.ml.worldcoin.org domain is no longer used.
            Backend::Analysis => "stage.orb",
            Backend::Local => todo!(),
        };

        let base = format!("https://fleet.{subdomain}.worldcoin.org/api/v2/orbs/");
        Self {
            status: concat_urls(&base, orb_id, "status"),
            keys_challenge: concat_urls(&base, orb_id, "keys/challenge"),
            keys_proof: concat_urls(&base, orb_id, "keys/proof"),
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
        assert_eq!(
            stage.keys_challenge.as_str(),
            "https://fleet.stage.orb.worldcoin.org/api/v2/orbs/ea2ea744/keys/challenge"
        );
        assert_eq!(
            prod.keys_proof.as_str(),
            "https://fleet.orb.worldcoin.org/api/v2/orbs/ea2ea744/keys/proof"
        );
    }

    #[test]
    fn test_analysis_backend_resolves_to_stage() {
        let orb_id = "ea2ea744".parse().unwrap();
        let analysis = Endpoints::new(Backend::Analysis, &orb_id);

        assert_eq!(
            analysis.status.as_str(),
            "https://fleet.stage.orb.worldcoin.org/api/v2/orbs/ea2ea744/status"
        );
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn test_local_backend_unimplemented() {
        let orb_id = "ea2ea744".parse().unwrap();
        let _local = Endpoints::new(Backend::Local, &orb_id);
    }
}
