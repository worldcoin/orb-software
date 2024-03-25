use std::fmt::Debug;
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use eyre::{eyre, Result, WrapErr};
use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

pub mod capture;

/// Handles offloading [`rodio::OutputStream`] to a separate thread. Kills the
/// stream on drop.
// TODO: Instead of one thread per stream, consider using a single thread that
// contains multiple streams.
struct StreamTask {
    // Fields are Options because we need to take ownership during drop.
    /// When dropped, kills the managed thread
    kill_signal: Option<std::sync::mpsc::SyncSender<()>>,
    task: Option<std::thread::JoinHandle<Result<()>>>,
}

impl StreamTask {
    fn new() -> Result<(Self, rodio::OutputStreamHandle)> {
        let (stream_send, stream_recv) = std::sync::mpsc::sync_channel(0);
        let (kill_send, kill_recv) = std::sync::mpsc::sync_channel(0);
        let task: std::thread::JoinHandle<Result<()>> = std::thread::Builder::new()
            .name("rodio stream thread".to_string())
            .spawn(move || -> Result<()> {
                // Get a output stream handle to the default physical sound device
                let (stream, stream_handle) = rodio::OutputStream::try_default()?;
                // Send it once to get it to the other thread
                stream_send
                    .send(stream_handle.clone())
                    .expect("should have sent handle");
                // Blocks until sender is dropped or sends data.
                let _ = kill_recv.recv();
                // This would have happened automatically but we are going to be
                // explicit about it.
                drop(stream);
                Ok(())
            })
            .wrap_err("failed to spawn stream task")?;
        let stream_handle = match stream_recv.recv() {
            Err(std::sync::mpsc::RecvError) => {
                task.join()
                    .map_err(|err| eyre!(format!("stream task panicked: {err:?}")))?
                    .wrap_err("stream task returned error")?;
                unreachable!(
                    "if we got a RecvError, should not be possible that the task did
                    not error",
                );
            }
            Ok(stream_handle) => stream_handle,
        };

        Ok((
            Self {
                kill_signal: Some(kill_send),
                task: Some(task),
            },
            stream_handle,
        ))
    }
}

impl Drop for StreamTask {
    fn drop(&mut self) {
        let kill_signal = self.kill_signal.take().unwrap();
        let task = self.task.take().unwrap();

        // Dropping signals the thread to exit
        drop(kill_signal);
        task.join()
            .expect("stream task should not have panicked")
            .expect("stream task should not have errored")
    }
}

pub const SOUNDS_DIR: &str = "/home/worldcoin/data/sounds";

/// Sound queue.
pub trait Player: Send + Sync {
    /// Loads sound files for the given language from the file system.
    fn load_sound_files(&self, language: Option<&str>) -> Result<()>;
    /// Queues a sound to be played.
    fn queue(&self, sound_type: Type);
    /// Finds the sound file path for the given sound type and sends it to the
    /// sound player only if the sink is empty
    fn try_queue(&self, sound_type: Type) -> Result<bool>;

    /// Sets the volume of the sound player, in percent.
    fn set_volume(&self, volume_percent: u64);

    /// Sets the language of the sound player.
    /// Format is en-US, en-GB, etc.
    fn set_language(&self, language: Option<&str>);
}

/// Available sound types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "sound_type", content = "value")]
pub enum Type {
    /// Sound type for voices.
    Voice(Voice),
    /// Sound type for melodies.
    Melody(Melody),
    /// Sound type for tests.
    VoiceTests(VoiceTests),
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
            /// Load sound files for a sound type.
            /// Paths to the sound files are stored in the given map.
            fn load_sound_files(
                directory: &str,
                sound_files: &DashMap<Type, Option<String>>,
                language: Option<&str>,
            ) -> Result<()> {
                $(
                    sound_files.insert(
                        Type::$name(Self::$sound),
                        load_filepaths(directory, $file, language),
                    );
                )*
                Ok(())
            }
        }
    };
}
pub(crate) use sound_enum;

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

/// Default sound volume
const DEFAULT_SOUND_VOLUME_PERCENT: u64 = 10;

pub struct Jetson {
    _stream_task: StreamTask,
    _stream_handle: rodio::OutputStreamHandle,
    queue_file: Mutex<mpsc::Sender<String>>,
    sound_files: DashMap<Type, Option<String>>,
    sink: Arc<rodio::Sink>,
}

/// Receives sound file paths and plays them.
async fn player(rx: &mut mpsc::Receiver<String>, sink: Arc<rodio::Sink>) {
    while let Some(sound_file) = rx.next().await {
        if let Ok(file) = File::open(sound_file.clone()) {
            if let Ok(decoder) = rodio::Decoder::new(BufReader::new(file)) {
                sink.append(decoder);
            } else {
                tracing::error!("Failed to decode sound file: {:?}", sound_file);
            }
        } else {
            tracing::error!("Failed to open sound file: {:?}", sound_file);
        }
    }
}

impl Jetson {
    pub fn spawn() -> Result<Self> {
        let (stream_task, stream_handle) =
            StreamTask::new().wrap_err("failed to create stream task")?;
        let sink = Arc::new(rodio::Sink::try_new(&stream_handle)?);
        let (tx, mut rx) = mpsc::channel(5);
        let sound = Self {
            _stream_task: stream_task,
            _stream_handle: stream_handle,
            queue_file: Mutex::new(tx),
            sound_files: DashMap::new(),
            sink: sink.clone(),
        };

        sound.load_sound_files(None)?;
        sound.set_volume(DEFAULT_SOUND_VOLUME_PERCENT);

        // spawn a task to play sounds in the background
        tokio::spawn(async move {
            player(&mut rx, sink).await;
            tracing::error!("Sound player task exited unexpectedly");
        });

        Ok(sound)
    }
}

impl Player for Jetson {
    fn load_sound_files(&self, language: Option<&str>) -> Result<()> {
        let language = language.map(ToOwned::to_owned);
        Voice::load_sound_files(SOUNDS_DIR, &self.sound_files, language.as_deref())?;
        Melody::load_sound_files(SOUNDS_DIR, &self.sound_files, language.as_deref())?;
        tracing::info!(
            "{} sound files loaded, for language {language:?}",
            self.sound_files.len(),
            language = language
        );
        Ok(())
    }

    /// Finds the sound file path for the given sound type and sends it to the
    /// sound player.
    fn queue(&self, sound_type: Type) {
        if let Some(sound_file) = self.sound_files.get(&sound_type) {
            if let Some(sound_file) = sound_file.value() {
                if let Ok(mut tx_queue) = self.queue_file.lock() {
                    if let Err(err) = tx_queue.try_send(sound_file.clone()) {
                        tracing::error!("Failed to queue sound: {:?}", err);
                    }
                }
            } else {
                tracing::error!(
                    "Sound file {:?} doesn't have a known file path",
                    sound_type
                );
            }
        } else {
            tracing::error!("Sound file not found: {:?}", sound_type);
        }
    }

    /// Finds the sound file path for the given sound type and sends it to the
    /// sound player only if the sink is empty
    fn try_queue(&self, sound_type: Type) -> Result<bool> {
        if self.sink.empty() {
            if let Some(sound_file) = self.sound_files.get(&sound_type) {
                if let Some(sound_file) = sound_file.value() {
                    if let Ok(mut tx_queue) = self.queue_file.lock() {
                        tx_queue
                            .try_send(sound_file.clone())
                            .wrap_err("Failed to queue sound")?;
                        Ok(true)
                    } else {
                        Err(eyre!("Failed to lock queue"))
                    }
                } else {
                    Err(eyre!(
                        "Sound file {:?} doesn't have a known file path",
                        sound_type
                    ))
                }
            } else {
                Err(eyre!("Sound file not found: {:?}", sound_type))
            }
        } else {
            Ok(false)
        }
    }

    fn set_volume(&self, volume_percent: u64) {
        self.sink
            .set_volume((volume_percent as f64 / 100_f64) as f32);
    }

    fn set_language(&self, language: Option<&str>) {
        self.sound_files.clear();
        let language = language.map(ToOwned::to_owned);

        if let Err(e) =
            Voice::load_sound_files(SOUNDS_DIR, &self.sound_files, language.as_deref())
        {
            tracing::error!("Failed to load voice sound files: {:?}", e);
        }

        if let Err(e) =
            Melody::load_sound_files(SOUNDS_DIR, &self.sound_files, language.as_deref())
        {
            tracing::error!("Failed to load melody sound files: {:?}", e);
        }
    }
}

fn load_filepaths(dir: &str, sound: &str, language: Option<&str>) -> Option<String> {
    // if a `language` is passed and the sound is a voice, make sure we append the
    // localized language to the file name
    // e.g. voice_server_error__es-ES.wav
    let has_extension = language.is_some()
        && !language.unwrap().contains("en-")
        && sound.contains("voice_");
    let lang_extension = if has_extension {
        if let Some(language) = language {
            format!("__{}", language)
        } else {
            "".to_string()
        }
    } else {
        "".to_string()
    };

    let path = format!("{}/{}{}.wav", dir, sound, lang_extension);
    match File::open(path.clone()) {
        Ok(_) => {
            tracing::debug!("Found sound file: {:?}", path);
            Some(path.clone())
        }
        Err(_) => {
            tracing::warn!("Sound file not found: {:?}", path);
            None
        }
    }
}

sound_enum! {
    pub enum VoiceTests {
        #[sound_enum(file = "voice_connected")]
        Connected,
    }
}

pub struct Fake {
    // We wrap this with a mutex even though we never access it, so that `Fake`
    // implements `Send + Sync`
    _stream_task: StreamTask,
    _stream_handle: rodio::OutputStreamHandle,
    queue_file: Mutex<mpsc::Sender<String>>,
    sound_files: DashMap<Type, Option<String>>,
    sink: Arc<rodio::Sink>,
}

impl Fake {
    #[allow(unused)]
    pub fn spawn() -> Result<Self> {
        // Get a output stream handle to the default physical sound device
        let (stream_task, stream_handle) =
            StreamTask::new().wrap_err("failed to create stream task")?;
        let sink = Arc::new(rodio::Sink::try_new(&stream_handle)?);
        let (tx, mut rx) = mpsc::channel(5);
        let sound = Self {
            _stream_task: stream_task,
            _stream_handle: stream_handle,
            queue_file: Mutex::new(tx),
            sound_files: DashMap::new(),
            sink: sink.clone(),
        };

        sound.load_sound_files(None)?;
        sound.set_volume(DEFAULT_SOUND_VOLUME_PERCENT);

        // spawn a task to play sounds in the background
        tokio::spawn(async move {
            player(&mut rx, sink).await;
        });

        Ok(sound)
    }
}

impl Player for Fake {
    fn load_sound_files(&self, language: Option<&str>) -> Result<()> {
        let sound_files = &self.sound_files.clone();
        let language = language.map(ToOwned::to_owned);
        VoiceTests::load_sound_files(
            "src/sound/tests",
            sound_files,
            language.as_deref(),
        )?;
        tracing::info!("Sound files for language {language:?} loaded successfully");
        Ok(())
    }

    /// Finds the sound file path for the given sound type and sends it to the
    /// sound player.
    fn queue(&self, sound_type: Type) {
        if let Some(sound_file) = self.sound_files.get(&sound_type) {
            if let Some(sound_file) = sound_file.value() {
                if let Ok(mut tx_queue) = self.queue_file.lock() {
                    if let Err(err) = tx_queue.try_send(sound_file.clone()) {
                        tracing::error!("Failed to queue sound: {:?}", err);
                    }
                }
            } else {
                tracing::error!(
                    "Sound file {:?} doesn't have a known file path",
                    sound_type
                );
            }
        } else {
            tracing::error!("Sound file not found: {:?}", sound_type);
        }
    }

    /// Finds the sound file path for the given sound type and sends it to the
    /// sound player **only if the sink is empty**
    fn try_queue(&self, sound_type: Type) -> Result<bool> {
        if self.sink.empty() {
            if let Some(sound_file) = self.sound_files.get(&sound_type) {
                if let Some(sound_file) = sound_file.value() {
                    if let Ok(mut tx_queue) = self.queue_file.lock() {
                        tx_queue
                            .try_send(sound_file.clone())
                            .wrap_err("Failed to queue sound")?;
                        return Ok(true);
                    }
                } else {
                    tracing::error!(
                        "Sound file {:?} doesn't have a known file path",
                        sound_type
                    );
                }
            } else {
                tracing::error!("Sound file not found: {:?}", sound_type);
            }
        }
        Ok(false)
    }

    fn set_volume(&self, volume_percent: u64) {
        self.sink
            .set_volume((volume_percent as f64 / 100_f64) as f32);
    }

    fn set_language(&self, language: Option<&str>) {
        self.sound_files.clear();
        if let Err(e) =
            VoiceTests::load_sound_files("src/sound/tests", &self.sound_files, language)
        {
            tracing::error!("Failed to load test sound files: {:?}", e);
        }
    }
}

// write tests to check if the sound files are loaded and played correctly
#[cfg(test)]
mod tests {
    use eyre::Context;

    use crate::sound::{Fake, Player, Type, VoiceTests};

    #[test]
    #[ignore = "Ignored due to sounds"] // test to run locally
    fn test_play_sound() {
        let sound = Fake::spawn().wrap_err("Failed to create sound").unwrap();

        sound.queue(Type::VoiceTests(VoiceTests::Connected));

        // delay to play the sound
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}
