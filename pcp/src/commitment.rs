//! Core-compatible generation of Hyrax commitments over caller-supplied bytes.
//!
//! This is generation, not verification of imported commitments. The pinned
//! implementation pads inputs to a power of two and emits no commitment rows for
//! inputs of 256 bytes or fewer. That legacy behavior is deliberately preserved.

use rand::{CryptoRng, RngCore};
use zeroize::Zeroizing;

/// Generated outputs bound to the immutable data used to compute them.
///
/// Blinding factors are sensitive and must only be disclosed to the intended
/// recipient. This wrapper clears its owned blinding bytes on drop; callers own
/// the lifetime and clearing of input data and any copies of the returned bytes.
pub struct Generated<'a> {
    data: &'a [u8],
    commitment: Vec<u8>,
    blinding_factors: Zeroizing<Vec<u8>>,
}

impl Generated<'_> {
    pub fn data(&self) -> &[u8] {
        self.data
    }

    pub fn commitment(&self) -> &[u8] {
        &self.commitment
    }

    pub fn blinding_factors(&self) -> &[u8] {
        &self.blinding_factors
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not generate Hyrax blinding seed")]
    Randomness(#[source] rand::Error),
}

/// Generates using the same pinned implementation and fresh 32-byte seed as core.
///
/// The caller supplies a cryptographically secure random source. Entropy failure
/// returns no result. Computation is synchronous and may be expensive; async
/// consumers must run it on their blocking worker. The local seed is cleared on
/// drop, but the upstream implementation does not clear all internal copies.
pub fn generate<'a>(
    data: &'a [u8],
    rng: &mut (impl RngCore + CryptoRng),
) -> Result<Generated<'a>, Error> {
    let mut seed = Zeroizing::new([0; 32]);
    rng.try_fill_bytes(seed.as_mut())
        .map_err(Error::Randomness)?;
    let output =
        hyrax::iriscode_commit::compute_commitments_binary_outputs(data, *seed);
    Ok(Generated {
        data,
        commitment: output.commitment_serialized,
        blinding_factors: Zeroizing::new(output.blinding_factors_serialized),
    })
}
