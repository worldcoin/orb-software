use eyre::Result;

// TODO: Figure out how to represent this type best.
#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub struct OrbFusedPubkey(());

impl OrbFusedPubkey {
    pub fn parse_pem(_pem: &str) -> Result<Self> {
        todo!()
    }
}
