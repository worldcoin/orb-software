use blake3::Hasher;

// Re-export the AppAuthenticatedData type in order to avoid version collisions
// when a third party crate also depends on orb_relay_messages directly.
pub use orb_relay_messages::common::v1::AppAuthenticatedData;

// Define the constant needed by the implementation
const PCP_VERSION_DEFAULT: u32 = 2;

/// Extension trait for [`AppAuthenticatedData`] that provides hashing and verification functionality.
///
/// This trait adds methods to hash and verify the authenticity of [`AppAuthenticatedData`]
/// instances using BLAKE3 cryptographic hashing.
pub trait AppAuthenticatedDataExt {
    /// Returns `true` if `hash` is a BLAKE3 hash of this [`AppAuthenticatedData`].
    ///
    /// This method calculates its own hash of the same length as the input
    /// `hash` and checks if both hashes are identical.
    fn verify(&self, hash: impl AsRef<[u8]>) -> bool;

    /// Calculates a BLAKE3 hash of the length `n`.
    fn hash(&self, n: usize) -> Vec<u8>;

    /// Updates the provided BLAKE3 hasher with all fields of this [`AppAuthenticatedData`].
    ///
    /// This method must hash every field in the struct to ensure complete validation.
    fn hasher_update(&self, hasher: &mut Hasher);
}

// Implement the trait for AppAuthenticatedData
impl AppAuthenticatedDataExt for AppAuthenticatedData {
    fn verify(&self, hash: impl AsRef<[u8]>) -> bool {
        let external_hash = hash.as_ref();
        let internal_hash = self.hash(external_hash.len());
        external_hash == internal_hash
    }

    fn hash(&self, n: usize) -> Vec<u8> {
        let mut hasher = Hasher::new();
        self.hasher_update(&mut hasher);
        let mut output = vec![0; n];
        hasher.finalize_xof().fill(&mut output);
        output
    }

    fn hasher_update(&self, hasher: &mut Hasher) {
        let Self {
            identity_commitment,
            self_custody_public_key,
            pcp_version,
            os_version,
            os,
        } = self;
        hasher.update(identity_commitment.as_bytes());
        hasher.update(self_custody_public_key.as_bytes());
        hasher.update(os_version.as_bytes());
        hasher.update(os.as_bytes());
        if *pcp_version != PCP_VERSION_DEFAULT {
            hasher.update(&pcp_version.to_le_bytes());
        }
    }
}
