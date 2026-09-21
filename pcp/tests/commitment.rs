use hyrax::iriscode_commit::compute_commitments_binary_outputs;
use orb_pcp::commitment::{self, Error};
use rand::{CryptoRng, RngCore, SeedableRng};

// Deterministic test double only; never a production entropy source.
struct SeedRng {
    seed: [u8; 32],
    calls: usize,
    fail: bool,
}

impl CryptoRng for SeedRng {}

impl RngCore for SeedRng {
    fn next_u32(&mut self) -> u32 {
        panic!("use fallible byte generation")
    }

    fn next_u64(&mut self) -> u64 {
        panic!("use fallible byte generation")
    }

    fn fill_bytes(&mut self, _: &mut [u8]) {
        panic!("use fallible byte generation")
    }

    fn try_fill_bytes(&mut self, bytes: &mut [u8]) -> Result<(), rand::Error> {
        assert_eq!(bytes.len(), 32);
        self.calls += 1;
        bytes.copy_from_slice(&self.seed);
        if self.fail {
            return Err(rand::Error::new(std::io::Error::other(
                "synthetic entropy failure",
            )));
        }
        Ok(())
    }
}

#[test]
fn exact_outputs_match_pinned_implementation_and_keep_original_data() {
    let seed = [0x5a; 32];
    for len in [257, 512, 513, 2048] {
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let expected = compute_commitments_binary_outputs(&data, seed);
        let mut rng = SeedRng {
            seed,
            calls: 0,
            fail: false,
        };
        let output = commitment::generate(&data, &mut rng).unwrap();
        assert_eq!(rng.calls, 1);
        assert_eq!(output.data(), data);
        assert_eq!(output.data().as_ptr(), data.as_ptr());
        assert_eq!(output.commitment(), expected.commitment_serialized);
        assert_eq!(
            output.blinding_factors(),
            expected.blinding_factors_serialized
        );
        assert!(!output.commitment().is_empty());
    }
}

#[test]
fn small_inputs_preserve_legacy_empty_outputs() {
    for len in [0, 1, 255, 256] {
        let data = vec![0x42; len];
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let output = commitment::generate(&data, &mut rng).unwrap();
        assert_eq!(output.data(), data);
        assert!(output.commitment().is_empty());
        assert!(output.blinding_factors().is_empty());
    }
}

#[test]
fn each_generation_draws_a_fresh_seed() {
    let data = [0x42; 512];
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let first = commitment::generate(&data, &mut rng).unwrap();
    let second = commitment::generate(&data, &mut rng).unwrap();
    assert_ne!(first.commitment(), second.commitment());
    assert_ne!(first.blinding_factors(), second.blinding_factors());
}

#[test]
fn entropy_failure_returns_no_generated_record_and_is_not_retried() {
    let mut rng = SeedRng {
        seed: [0x5a; 32],
        calls: 0,
        fail: true,
    };
    let result = commitment::generate(&[0x42; 512], &mut rng);
    assert!(matches!(result, Err(Error::Randomness(_))));
    assert_eq!(rng.calls, 1);
}
