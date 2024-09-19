//! Audio support.

pub(crate) mod capture;

use dashmap::DashMap;
use eyre::{Result, WrapErr};
use futures::prelude::*;
use orb_sound::{Queue, SoundBuilder};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::{fmt, io::Cursor, path::Path, pin::Pin, sync::Arc};
use tokio::fs;

/// ALSA sound card name.
const SOUND_CARD_NAME: &str = "default";
/// Path to the directory with the sound files.
const SOUNDS_DIR: &str = "/home/worldcoin/data/sounds";
/// Default master volume level.
const DEFAULT_MASTER_VOLUME: f64 = 0.15;

/// Sound queue.
pub trait Player: fmt::Debug + Send {
    /// Loads sound files for the given language from the file system.
    fn load_sound_files(
        &self,
        language: Option<&str>,
        ignore_missing_sounds: bool,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    /// Creates a new sound builder object.
    fn build(&mut self, sound_type: Type) -> Result<SoundBuilder>;

    /// Returns a new handler to the shared queue.
    fn clone(&self) -> Box<dyn Player>;

    /// Sets the master volume.
    fn set_master_volume(&mut self, level: u64);

    /// Queues a sound to be played.
    /// Helper method for `build` and `push`.
    /// Optionally delays the sound.
    fn queue(&mut self, sound_type: Type, delay: Option<Duration>) -> Result<()>;

    /// Queues a sound to be played with a max delay.
    /// Helper method for `build` and `push`.
    fn try_queue(&mut self, sound_type: Type) -> Result<bool>;
}

/// Sound queue for the Orb hardware.
pub struct Jetson {
    queue: Arc<Queue>,
    sound_files: Arc<DashMap<Type, SoundFile>>,
    volume: f64,
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
                language: Option<&str>,
                ignore_missing_sounds: bool,
            ) -> Result<()> {
                $(
                    sound_files.insert(
                        Type::$name(Self::$sound),
                        load_sound_file($file, language, ignore_missing_sounds).await?,
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
        #[sound_enum(file = "sound_success_alt1_chime1")]
        UserQrLoadSuccessSelfServe,
        #[sound_enum(file = "sound_success_alt1_chime2")]
        UserBiometricCaptureStartSelfServe,
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
struct SoundFile(Arc<Vec<u8>>);

impl AsRef<[u8]> for SoundFile {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Jetson {
    /// Spawns a new sound queue.
    pub async fn spawn() -> Result<Self> {
        let sound = Self {
            queue: Arc::new(Queue::spawn(SOUND_CARD_NAME)?),
            sound_files: Arc::new(DashMap::new()),
            volume: DEFAULT_MASTER_VOLUME,
        };
        let language = Some("EN-en");
        sound.load_sound_files(language, true).await?;
        Ok(sound)
    }
}

impl Player for Jetson {
    fn load_sound_files(
        &self,
        language: Option<&str>,
        ignore_missing_sounds: bool,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        let sound_files = Arc::clone(&self.sound_files);
        let language = language.map(ToOwned::to_owned);
        Box::pin(async move {
            Voice::load_sound_files(
                &sound_files,
                language.as_deref(),
                ignore_missing_sounds,
            )
            .await?;
            Melody::load_sound_files(
                &sound_files,
                language.as_deref(),
                ignore_missing_sounds,
            )
            .await?;
            let count = sound_files.len();
            tracing::info!("Sound files for language {language:?} loaded successfully ({count:?} files)");
            Ok(())
        })
    }

    #[allow(clippy::missing_panics_doc)]
    fn build(&mut self, sound_type: Type) -> Result<SoundBuilder> {
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

    fn set_master_volume(&mut self, level: u64) {
        self.volume = level as f64 / 100.0;
    }

    fn queue(&mut self, sound_type: Type, delay: Option<Duration>) -> Result<()> {
        let volume = self.volume;
        self.build(sound_type)?.volume(volume).delay(delay).push()?;
        Ok(())
    }

    fn try_queue(&mut self, sound_type: Type) -> Result<bool> {
        if self.queue.empty() {
            self.queue(sound_type, None)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Returns SoundFile if sound in filesystem entries.
async fn load_sound_file(
    sound: &str,
    language: Option<&str>,
    ignore_missing: bool,
) -> Result<SoundFile> {
    let sounds_dir = Path::new(SOUNDS_DIR);
    if let Some(language) = language {
        let file = sounds_dir.join(format!("{sound}__{language}.wav"));
        if file.exists() {
            let data = fs::read(&file)
                .await
                .wrap_err_with(|| format!("failed to read {}", file.display()))?;
            return Ok(SoundFile(Arc::new(data)));
        }
    }
    let file = sounds_dir.join(format!("{sound}.wav"));
    let data = match fs::read(&file)
        .await
        .wrap_err_with(|| format!("failed to read {}", file.display()))
    {
        Ok(data) => data,
        Err(err) => {
            if ignore_missing {
                tracing::error!("Ignoring missing sounds: {err}");
                Vec::new()
            } else {
                return Err(err);
            }
        }
    };
    Ok(SoundFile(Arc::new(data)))
}

impl fmt::Debug for Jetson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sound").finish()
    }
}
