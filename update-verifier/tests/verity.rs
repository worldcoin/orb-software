#[cfg(target_os = "linux")]
mod fixture;

#[cfg(target_os = "linux")]
mod test_verity {
    use super::fixture::{DiamondFixture, PearlFixture};
    use orb_info::orb_os_release::OrbOsPlatform;
    use orb_update_verifier::verity::{self, VerityError};

    #[test]
    fn validates_diamond_verity_image() {
        let fixture = DiamondFixture::new();
        verity::validate_verity(OrbOsPlatform::Diamond, &fixture.source_root).unwrap();
    }

    #[test]
    fn rejects_corrupted_diamond_verity_image() {
        let fixture = DiamondFixture::new().corrupt();

        assert!(matches!(
            verity::validate_verity(OrbOsPlatform::Diamond, &fixture.source_root),
            Err(VerityError::VerificationFailed(..))
        ));
    }

    #[test]
    fn validates_pearl_verity_images() {
        let fixture = PearlFixture::new();

        verity::validate_verity(OrbOsPlatform::Pearl, &fixture.source_root).unwrap();
    }

    #[test]
    fn rejects_corrupted_pearl_system_verity_image() {
        let fixture = PearlFixture::new().corrupt_system_image();

        assert!(matches!(
            verity::validate_verity(OrbOsPlatform::Pearl, &fixture.source_root),
            Err(VerityError::VerificationFailed(..))
        ));
    }
}
