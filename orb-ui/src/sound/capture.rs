use crate::sound;

/// Loop sounds during biometric capture
/// divided into 3 loops with 3 parts each
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureLoopSound {
    Loop01(LoopSoundPart),
    Loop02(LoopSoundPart),
    Loop03(LoopSoundPart),
}

/// Loop sound sub-parts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopSoundPart {
    PartA,
    PartB,
    PartC,
}

impl Default for CaptureLoopSound {
    fn default() -> Self {
        CaptureLoopSound::Loop01(LoopSoundPart::PartA)
    }
}

impl CaptureLoopSound {
    /// Come back to part A of the current playing loop sound
    pub fn restart_current_loop(&mut self) {
        match self {
            CaptureLoopSound::Loop01(_) => {
                *self = CaptureLoopSound::Loop01(LoopSoundPart::PartA);
            }
            CaptureLoopSound::Loop02(_) => {
                *self = CaptureLoopSound::Loop02(LoopSoundPart::PartA);
            }
            CaptureLoopSound::Loop03(_) => {
                *self = CaptureLoopSound::Loop03(LoopSoundPart::PartA);
            }
        }
    }

    /// Reset to the first loop sound
    pub fn reset(&mut self) {
        *self = CaptureLoopSound::Loop01(LoopSoundPart::PartA);
    }
}

impl Iterator for CaptureLoopSound {
    type Item = sound::Melody;

    /// Iterate over loop sounds, following the pattern:
    ///     - 01: 3 parts A, B, C
    ///     - then 02 & 03 interchangeably
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            CaptureLoopSound::Loop01(part) => match part {
                LoopSoundPart::PartA => {
                    *self = CaptureLoopSound::Loop01(LoopSoundPart::PartB);
                    Some(sound::Melody::IrisScanningLoop01A)
                }
                LoopSoundPart::PartB => {
                    *self = CaptureLoopSound::Loop01(LoopSoundPart::PartC);
                    Some(sound::Melody::IrisScanningLoop01B)
                }
                LoopSoundPart::PartC => {
                    *self = CaptureLoopSound::Loop02(LoopSoundPart::PartA);
                    Some(sound::Melody::IrisScanningLoop01C)
                }
            },
            CaptureLoopSound::Loop02(part) => match part {
                LoopSoundPart::PartA => {
                    *self = CaptureLoopSound::Loop02(LoopSoundPart::PartB);
                    Some(sound::Melody::IrisScanningLoop02A)
                }
                LoopSoundPart::PartB => {
                    *self = CaptureLoopSound::Loop02(LoopSoundPart::PartC);
                    Some(sound::Melody::IrisScanningLoop02B)
                }
                LoopSoundPart::PartC => {
                    *self = CaptureLoopSound::Loop03(LoopSoundPart::PartA);
                    Some(sound::Melody::IrisScanningLoop02C)
                }
            },
            CaptureLoopSound::Loop03(part) => match part {
                LoopSoundPart::PartA => {
                    *self = CaptureLoopSound::Loop03(LoopSoundPart::PartB);
                    Some(sound::Melody::IrisScanningLoop03A)
                }
                LoopSoundPart::PartB => {
                    *self = CaptureLoopSound::Loop03(LoopSoundPart::PartC);
                    Some(sound::Melody::IrisScanningLoop03B)
                }
                LoopSoundPart::PartC => {
                    *self = CaptureLoopSound::Loop02(LoopSoundPart::PartA);
                    Some(sound::Melody::IrisScanningLoop03C)
                }
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// we want to test the iterator implementation
    #[test]
    fn test_capture_loop_sound() {
        let mut loop_sound = CaptureLoopSound::default();
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop01A));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop01B));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop01C));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop02A));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop02B));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop02C));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop03A));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop03B));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop03C));
        // to loop 2
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop02A));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop02B));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop02C));
        // to loop 3
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop03A));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop03B));
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop03C));
    }

    /// we want to test the out_of_range method
    #[test]
    fn test_out_of_range() {
        let mut loop_sound = CaptureLoopSound::default();
        assert_eq!(loop_sound.next(), Some(sound::Melody::IrisScanningLoop01A));
        assert_eq!(loop_sound, CaptureLoopSound::Loop01(LoopSoundPart::PartB));
        loop_sound.restart_current_loop();
        assert_eq!(loop_sound, CaptureLoopSound::Loop01(LoopSoundPart::PartA));

        let mut loop_sound = CaptureLoopSound::Loop02(LoopSoundPart::PartC);
        loop_sound.restart_current_loop();
        assert_eq!(loop_sound, CaptureLoopSound::Loop02(LoopSoundPart::PartA));

        let mut loop_sound = CaptureLoopSound::Loop03(LoopSoundPart::PartC);
        loop_sound.restart_current_loop();
        assert_eq!(loop_sound, CaptureLoopSound::Loop03(LoopSoundPart::PartA));
    }
}
