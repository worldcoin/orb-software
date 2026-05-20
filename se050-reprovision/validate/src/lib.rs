mod base64_serde;
mod cert;
mod challenge;
mod orb_fused_pubkey;
mod proof;

use bon::Builder;
use derive_more::Deref;
use eyre::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

pub use crate::cert::OrbNxpCert;
pub use crate::challenge::Nonce;
pub use crate::orb_fused_pubkey::OrbFusedPubkey;
pub use crate::proof::{KeyInfo, Proof};

/// Generates a new challenge. Care should be taken by the backend to ensure
/// the following, as the security hinges on these guarantees:
///
/// * VERY IMPORTANT: The nxp cert and the jetson pubkey *must* have already been
///   blessed at manufacturing time and known to originate from the same physical orb.
///   This cannot be taken on faith, we must check our manufacturing database to know
///   for sure. The entire trust of the system relies on this prerequisite, dont forget
///   it! VERY IMPORANT!!
/// * The nonce of this challenge is always a new, unique value and doesn't collide with
///   any of the past nonces sent by the backend.
#[derive(Debug, Eq, PartialEq, Clone, Hash, Builder)]
#[builder(finish_fn =
    i_wont_call_this_unless_i_read_the_security_requirements
)]
pub struct Challenge {
    orb_nxp_cert: OrbNxpCert,
    orb_fused_pubkey: OrbFusedPubkey,
    nonce: Nonce,
}

impl Challenge {
    /// Validates the proof.
    pub fn validate(&self, proof: &Proof) -> Result<ValidatedProof> {
        let proof_jetson_authkey = OrbFusedPubkey::parse_pem(&proof.jetson_authkey.key)
            .wrap_err("failed to parse OrbFusedPubkey from proof's jetson_authkey")?;
        ensure!(
            self.orb_fused_pubkey == proof_jetson_authkey,
            "orb fused pubkey did not match proof"
        );

        ensure!(
            self.nonce == proof.server_nonce,
            "challenge did not match proof"
        );

        todo!(
            "wait for more se050 crate changes to be merged for the rest of the
            implementation"
        )
    }
}

/// A validated [`Proof`]. Now that it has been validated, we have guarantees of the
/// following:
///
/// 1. This is a legitimate orb. If we wanted to we could issue a short lived token
///    now, even without going through orb-attest.
/// 2. The nxp se050 and the jetson are "paired" (at least they were at the time of
///    the proof).
/// 3. The proof was lively (i.e. recent and not a replay of old data).
/// 4. No one is man-in-the-middling the se050 and jetson.
/// 5. All communication between the se050 and jetson is encrypted, including against
///    sniffing at the physical layer.
///
/// We don't have any way of knowing the following:
/// 1. Either or both of the nxp or jetson may have been desoldered and put on a server
///    farm.
/// 2. The nxp or jetson may have been stolen.
/// 3. The jetson may have been hacked.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Deref)]
#[serde(transparent)]
pub struct ValidatedProof(Proof);
