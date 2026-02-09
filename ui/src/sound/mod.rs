#![allow(clippy::uninlined_format_args)]
//! Audio support.

pub(crate) mod capture;

use dashmap::DashMap;
use futures::prelude::*;
use orb_sound::{Queue, SoundBuilder};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::time::Duration;
use std::{fmt, io::Cursor, path::Path, pin::Pin, sync::Arc};
use tokio::fs;

/// ALSA sound card name.
const SOUND_CARD_NAME: &str = "default";
/// Path to the directory with the sound files.
const SOUNDS_DIR: &str = "/home/worldcoin/data/sounds";
const DEFAULT_LANGUAGE: Language = Language::En;
/// Default master volume level.
const DEFAULT_MASTER_VOLUME: f64 = 0.15;

/// Sound queue.
pub trait Player: fmt::Debug + Send {
    /// Loads sound files for the given language from the file system.
    fn load_sound_files(
        &self,
        config: SoundConfig,
    ) -> Pin<Box<dyn Future<Output = Result<(), SoundError>> + Send + '_>>;

    /// Creates a new sound builder object.
    fn build(&mut self, sound_type: Type) -> eyre::Result<SoundBuilder<'_>>;

    /// Returns a new handler to the shared queue.
    fn clone(&self) -> Box<dyn Player>;

    /// Returns the master volume [0, 100].
    fn volume(&self) -> u64;

    /// Sets the master volume.
    fn set_master_volume(&mut self, level: u64);

    /// Queues a sound to be played.
    /// Helper method for `build` and `push`.
    /// Optionally delays the sound.
    fn queue(&mut self, sound_type: Type, delay: Duration) -> eyre::Result<()>;

    /// Queues a sound to be played with a max delay.
    /// Helper method for `build` and `push`.
    fn try_queue(&mut self, sound_type: Type) -> eyre::Result<bool>;

    fn get_duration(&self, sound_type: Type) -> Option<Duration>;
}

/// Sound queue for the Orb hardware.
pub struct Jetson {
    queue: Arc<Queue>,
    sound_files: Arc<DashMap<Type, SoundFile>>,
    volume: f64,
}

#[derive(Debug)]
pub enum SoundError {
    MissingFile(String),
    NoSuchDirectory(String),
    UnsupportedLanguage,
    UnsupportedSoundFormat(String),
    OsError,
}

#[derive(Clone, Debug)]
pub enum Language {
    /// English
    En,
    /// Spanish
    Es,
}

impl TryFrom<String> for Language {
    type Error = SoundError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.to_lowercase().as_str().contains("en") {
            Ok(Language::En)
        } else if value.to_lowercase().as_str().contains("es") {
            Ok(Language::Es)
        } else {
            Err(SoundError::UnsupportedLanguage)
        }
    }
}

impl Display for Language {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::En => write!(f, "en-EN"),
            Language::Es => write!(f, "es-ES"),
        }
    }
}

impl Language {
    /// Returns the file suffix for the language.
    ///
    pub fn as_suffix(&self) -> &str {
        match self {
            Language::En => "",
            Language::Es => "__es-ES",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SoundConfig {
    sound_path: String,
    language: Language,
    /// don't load sounds that are missing, nothing will be played
    /// if false, an error will be raised, and default sounds will be used if it exists (EN-en)
    ignore_missing_sounds: bool,
}

impl SoundConfig {
    /// Set language, use default if None
    pub(crate) fn with_language(
        mut self,
        lang: Option<&str>,
    ) -> Result<Self, SoundError> {
        if let Some(lang) = lang {
            self.language = Language::try_from(lang.to_string())?;
        } else {
            self.language = DEFAULT_LANGUAGE;
        }
        Ok(self)
    }

    #[cfg(test)]
    pub(crate) fn with_path(mut self, path: &str) -> Result<Self, SoundError> {
        if !Path::new(path).exists() {
            return Err(SoundError::NoSuchDirectory(path.to_string()));
        }
        self.sound_path = path.to_string();
        Ok(self)
    }

    #[cfg(test)]
    pub(crate) fn with_ignore_missing_sounds(mut self, ignore: bool) -> Self {
        self.ignore_missing_sounds = ignore;
        self
    }
}

impl Default for SoundConfig {
    fn default() -> SoundConfig {
        SoundConfig {
            sound_path: SOUNDS_DIR.to_string(),
            language: Language::En,
            ignore_missing_sounds: true,
        }
    }
}

/// Available sound types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "sound_type", content = "value")]
pub enum Type {
    /// Sound type for voices.
    Voice(Voice),
    /// Sound type for melodies.
    Melody(Melody),
}

macro_rules! sound_enum {
    (
        $(#[$($enum_attrs:tt)*])*
        $vis:vis enum $name:ident {
            $(
                #[sound_enum(file = $file:expr)]
                $(#[$($sound_attrs:tt)*])*
                $sound:ident,
            )*
        }
    ) => {
        $(#[$($enum_attrs)*])*
        #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
        $vis enum $name {
            $(
                $(#[$($sound_attrs)*])*
                $sound,
            )*
        }

        impl $name {
            async fn load_sound_files(
                sound_files: &DashMap<Type, SoundFile>,
                config: &SoundConfig,
            ) -> Result<(), SoundError> {
                $(
                    sound_files.insert(
                        Type::$name(Self::$sound),
                        load_sound_file($file, config).await?,
                    );
                )*
                Ok(())
            }
        }
    };
}

sound_enum! {
    /// Available voices.
    #[allow(missing_docs)]
    pub enum Voice {
        #[sound_enum(file = "silence")]
        Silence,
        #[sound_enum(file = "voice_show_wifi_hotspot_qr_code")]
        ShowWifiHotspotQrCode,
        #[sound_enum(file = "voice_iris_move_farther")]
        MoveFarther,
        #[sound_enum(file = "voice_iris_move_closer")]
        MoveCloser,
        #[sound_enum(file = "voice_overheating")]
        Overheating,
        #[sound_enum(file = "voice_please_put_the_calibration_target_in_the_frame")]
        PutCalibrationTarget,
        #[sound_enum(file = "voice_whole_pattern_is_visible")]
        CalibrationTargetVisible,
        #[sound_enum(file = "voice_please_do_not_move_the_calibration_target")]
        DoNotMoveCalibrationTarget,
        #[sound_enum(file = "voice_verification_not_successful_please_try_again")]
        VerificationNotSuccessfulPleaseTryAgain,
        #[sound_enum(file = "voice_qr_code_invalid")]
        QrCodeInvalid,
        #[sound_enum(file = "voice_internet_connection_too_slow_to_perform_signups")]
        InternetConnectionTooSlowToPerformSignups,
        #[sound_enum(file = "voice_internet_connection_too_slow_signups_might_take_longer_than_expected")]
        InternetConnectionTooSlowSignupsMightTakeLonger,
        #[sound_enum(file = "voice_wrong_qr_code_format")]
        WrongQrCodeFormat,
        #[sound_enum(file = "voice_timeout")]
        Timeout,
        #[sound_enum(file = "voice_server_error")]
        ServerError,
        #[sound_enum(file = "voice_face_not_found")]
        FaceNotFound,
        #[sound_enum(file = "voice_test_firmware_warning")]
        TestFirmwareWarning,
        #[sound_enum(file = "voice_please_do_not_shutdown")]
        PleaseDontShutDown,
        #[sound_enum(file = "voice_iris_open")]
        OpenEyes,
        #[sound_enum(file = "voice_face_not_seen")]
        FaceNotSeen,
        #[sound_enum(file = "voice_face_too_far")]
        FaceTooFar,
        #[sound_enum(file = "voice_face_too_close")]
        FaceTooClose,
        #[sound_enum(file = "voice_face_too_low")]
        FaceTooLow,
        #[sound_enum(file = "voice_face_too_high")]
        FaceTooHigh,
        #[sound_enum(file = "voice_position_not_stable")]
        PositionNotStable,
        #[sound_enum(file = "voice_face_occluded")]
        FaceOccluded,
        #[sound_enum(file = "voice_warming_up")]
        WarmingUp,
        #[sound_enum(file = "voice_face_glasses")]
        FaceGlasses,
        #[sound_enum(file = "voice_face_mask")]
        FaceMask,
        #[sound_enum(file = "voice_hair_occlusion")]
        HairOcclusion,
        #[sound_enum(file = "voice_eye_occlusion")]
        EyeOcclusion,
    }
}

sound_enum! {
    /// Available melodies.
    #[allow(missing_docs)]
    pub enum Melody {
        #[sound_enum(file = "sound_bootup")]
        BootUp,
        #[sound_enum(file = "sound_powering_down")]
        PoweringDown,
        #[sound_enum(file = "sound_qr_code_capture")]
        QrCodeCapture,
        #[sound_enum(file = "sound_signup_success")]
        SignupSuccess,
        #[sound_enum(file = "sound_overheating")]
        Overheating, // TODO: Play when the overheating logic is implemented.
        #[sound_enum(file = "sound_internet_connection_successful")]
        InternetConnectionSuccessful,
        #[sound_enum(file = "sound_qr_load_success")]
        QrLoadSuccess,
        #[sound_enum(file = "sound_user_qr_load_success")]
        UserQrLoadSuccess,
        #[sound_enum(file = "sound_user_start_capture")]
        UserStartCapture,
        #[sound_enum(file = "sound_iris_scan_success")]
        IrisScanSuccess,
        #[sound_enum(file = "sound_error")]
        SoundError,
        #[sound_enum(file = "sound_start_signup")]
        StartSignup,
        #[sound_enum(file = "sound_iris_scanning_loop_01A")]
        IrisScanningLoop01A,
        #[sound_enum(file = "sound_iris_scanning_loop_01B")]
        IrisScanningLoop01B,
        #[sound_enum(file = "sound_iris_scanning_loop_01C")]
        IrisScanningLoop01C,
        #[sound_enum(file = "sound_iris_scanning_loop_02A")]
        IrisScanningLoop02A,
        #[sound_enum(file = "sound_iris_scanning_loop_02B")]
        IrisScanningLoop02B,
        #[sound_enum(file = "sound_iris_scanning_loop_02C")]
        IrisScanningLoop02C,
        #[sound_enum(file = "sound_iris_scanning_loop_03A")]
        IrisScanningLoop03A,
        #[sound_enum(file = "sound_iris_scanning_loop_03B")]
        IrisScanningLoop03B,
        #[sound_enum(file = "sound_iris_scanning_loop_03C")]
        IrisScanningLoop03C,
        #[sound_enum(file = "sound_start_idle")]
        StartIdle,
    }
}

#[derive(Clone)]
struct SoundFile {
    data: Arc<Vec<u8>>,
    pub duration: Duration,
}

impl AsRef<[u8]> for SoundFile {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Jetson {
    /// Spawns a new sound queue.
    pub async fn spawn() -> Result<Self, SoundError> {
        let sound = Self {
            queue: Arc::new(
                Queue::spawn(SOUND_CARD_NAME).map_err(|_| SoundError::OsError)?,
            ),
            sound_files: Arc::new(DashMap::new()),
            volume: DEFAULT_MASTER_VOLUME,
        };
        let config = SoundConfig::default();
        sound.load_sound_files(config).await?;
        Ok(sound)
    }
}

impl Player for Jetson {
    fn load_sound_files(
        &self,
        config: SoundConfig,
    ) -> Pin<Box<dyn Future<Output = Result<(), SoundError>> + Send + '_>> {
        let sound_files = Arc::clone(&self.sound_files);
        Box::pin(async move {
            Voice::load_sound_files(&sound_files, &config.clone()).await?;
            Melody::load_sound_files(&sound_files, &config.clone()).await?;
            let count = sound_files.len();
            tracing::debug!(
                "Sound files for language {:?} loaded successfully ({count:?} files)",
                config.language
            );
            Ok(())
        })
    }

    #[allow(clippy::missing_panics_doc)]
    fn build(&mut self, sound_type: Type) -> eyre::Result<SoundBuilder<'_>> {
        let sound_file = self.sound_files.get(&sound_type).unwrap();
        // It does Arc::clone under the hood, which is cheap.
        let reader =
            (!sound_file.as_ref().is_empty()).then(|| Cursor::new(sound_file.clone()));
        Ok(self.queue.sound(reader, format!("{sound_type:?}")))
    }

    fn clone(&self) -> Box<dyn Player> {
        Box::new(Jetson {
            queue: self.queue.clone(),
            sound_files: self.sound_files.clone(),
            volume: self.volume,
        })
    }

    fn volume(&self) -> u64 {
        (self.volume * 100.0) as u64
    }

    fn set_master_volume(&mut self, level: u64) {
        self.volume = level as f64 / 100.0;
    }

    fn queue(&mut self, sound_type: Type, delay: Duration) -> eyre::Result<()> {
        let volume = self.volume;
        self.build(sound_type)?.volume(volume).delay(delay).push()?;
        Ok(())
    }

    fn try_queue(&mut self, sound_type: Type) -> eyre::Result<bool> {
        if self.queue.empty() {
            self.queue(sound_type, Duration::ZERO)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get_duration(&self, sound_type: Type) -> Option<Duration> {
        self.sound_files
            .get(&sound_type)
            .map(|sound_file| sound_file.duration)
    }
}

/// Returns SoundFile if sound in filesystem entries.
async fn load_sound_file(
    filename: &str,
    config: &SoundConfig,
) -> Result<SoundFile, SoundError> {
    let sounds_dir = Path::new(config.sound_path.as_str());

    // voices have language (other than english) appended to the file name
    // sounds don't have any language
    let voice_filename = format!("{filename}{}.wav", config.language.as_suffix());
    let file = match sounds_dir.join(voice_filename.clone()).exists() {
        true => sounds_dir.join(voice_filename),
        false => sounds_dir.join(format!("{filename}.wav")),
    };

    let data = match fs::read(&file).await {
        Ok(d) => d,
        Err(e) => {
            if config.ignore_missing_sounds {
                tracing::error!("ignoring missing sound: {e}");
                Vec::new()
            } else {
                return Err(SoundError::MissingFile(e.to_string()));
            }
        }
    };

    let duration = {
        let reader = hound::WavReader::open(&file).map_err(|_| {
            SoundError::MissingFile(String::from(file.to_str().unwrap()))
        })?;
        Duration::from_secs_f64(
            f64::from(reader.duration()) / f64::from(reader.spec().sample_rate),
        )
    };

    // we have had errors with reading files encoded over 24 bits, so
    // this test ensure that wav files are sampled on 16 bits, for full Jetson compatibility.
    // remove this test if different sampling are supported.
    #[cfg(test)]
    {
        let reader = hound::WavReader::open(&file).map_err(|_| {
            SoundError::MissingFile(String::from(file.to_str().unwrap()))
        })?;
        assert_eq!(
            reader.spec().bits_per_sample,
            16,
            "Only 16-bit sounds are supported: {:?}",
            &file
        );
    }

    Ok(SoundFile {
        data: Arc::new(data),
        duration,
    })
}

impl fmt::Debug for Jetson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sound").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{Melody, Player, SoundConfig, SoundError, SoundFile, Type, Voice};
    use dashmap::DashMap;
    use orb_sound::SoundBuilder;
    use std::fmt::{Debug, Formatter};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    struct MockJetson {
        sound_files: Arc<DashMap<Type, SoundFile>>,
    }

    impl Debug for MockJetson {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("MockJetson").finish()
        }
    }

    impl Player for MockJetson {
        fn load_sound_files(
            &self,
            config: SoundConfig,
        ) -> Pin<Box<dyn Future<Output = eyre::Result<(), SoundError>> + Send + '_>>
        {
            let sound_files = Arc::clone(&self.sound_files);
            Box::pin(async move {
                Voice::load_sound_files(&sound_files, &config).await?;
                Melody::load_sound_files(&sound_files, &config).await?;
                let count = sound_files.len();
                tracing::debug!("Sound files for language {:?} loaded successfully ({count:?} files)", config.language);
                Ok(())
            })
        }

        fn build(&mut self, _sound_type: Type) -> eyre::Result<SoundBuilder<'_>> {
            unimplemented!()
        }

        fn clone(&self) -> Box<dyn Player> {
            Box::new(MockJetson {
                sound_files: self.sound_files.clone(),
            })
        }

        fn volume(&self) -> u64 {
            unimplemented!()
        }

        fn set_master_volume(&mut self, _level: u64) {
            unimplemented!()
        }

        fn queue(&mut self, _sound_type: Type, _delay: Duration) -> eyre::Result<()> {
            unimplemented!()
        }

        fn try_queue(&mut self, _sound_type: Type) -> eyre::Result<bool> {
            unimplemented!()
        }

        fn get_duration(&self, _sound_type: Type) -> Option<Duration> {
            unimplemented!()
        }
    }

    /// This test allows us to check that all files that can be pulled by the UI
    /// are present in the repository and are all encoded over 16 bits
    #[tokio::test]
    async fn test_load_sound_file() -> Result<(), SoundError> {
        let sound = MockJetson {
            sound_files: Arc::new(DashMap::new()),
        };

        let config = SoundConfig::default()
            .with_language(None)?
            .with_path(concat!(env!("CARGO_MANIFEST_DIR"), "/sound/assets"))?
            .with_ignore_missing_sounds(false);
        let res = sound.load_sound_files(config).await;
        if let Err(e) = &res {
            println!("{:?}", e);
        }
        assert!(res.is_ok(), "Default (None) failed");

        let config = SoundConfig::default()
            .with_language(Some("en-EN"))?
            .with_path(concat!(env!("CARGO_MANIFEST_DIR"), "/sound/assets"))?
            .with_ignore_missing_sounds(false);
        let res = sound.load_sound_files(config).await;
        assert!(res.is_ok(), "en-EN failed: {:?}", res);

        let config = SoundConfig::default()
            .with_language(Some("es-ES"))?
            .with_path(concat!(env!("CARGO_MANIFEST_DIR"), "/sound/assets"))?
            .with_ignore_missing_sounds(false);
        let res = sound.load_sound_files(config).await;
        assert!(res.is_ok(), "es-ES failed: {:?}", res);

        // unsupported / missing voice files
        let config = SoundConfig::default().with_language(Some("fr-FR"));
        assert!(config.is_err(), "fr-FR should have failed");

        let config = SoundConfig::default().with_path("doesnotexist");
        assert!(
            config.is_err(),
            "path that don't exist should throw an error"
        );

        Ok(())
    }
}
