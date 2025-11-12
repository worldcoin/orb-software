#![allow(clippy::borrow_as_ptr)]
use super::{alsa_to_io_error, Access, AlsaResult, Format, HwParams, ToAlsaResult};
use alsa_sys::{
    snd_pcm_bytes_to_frames, snd_pcm_close, snd_pcm_drain, snd_pcm_drop,
    snd_pcm_frames_to_bytes, snd_pcm_hw_params, snd_pcm_open, snd_pcm_pause,
    snd_pcm_prepare, snd_pcm_recover, snd_pcm_reset, snd_pcm_resume, snd_pcm_start,
    snd_pcm_state, snd_pcm_state_t, snd_pcm_t, snd_pcm_writei,
    SND_PCM_STATE_DISCONNECTED, SND_PCM_STATE_DRAINING, SND_PCM_STATE_OPEN,
    SND_PCM_STATE_PAUSED, SND_PCM_STATE_PREPARED, SND_PCM_STATE_RUNNING,
    SND_PCM_STATE_SETUP, SND_PCM_STATE_SUSPENDED, SND_PCM_STATE_XRUN,
    SND_PCM_STREAM_PLAYBACK,
};
use libc::EPIPE;
use std::{ffi::CString, io, io::prelude::*, ptr, thread::sleep, time::Duration};

const WAV_FORMAT_PCM: u16 = 0x01;
const WAV_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// PCM handle.
pub struct Device {
    snd_pcm: *mut snd_pcm_t,
}

unsafe impl Send for Device {}

/// PCM state.
#[derive(Clone, Copy, Debug)]
pub enum State {
    /// Open.
    Open,
    /// Setup installed.
    Setup,
    /// Ready to start.
    Prepared,
    /// Running.
    Running,
    /// Stopped: underrun (playback) or overrun (capture) detected.
    Xrun,
    /// Draining: running (playback) or stopped (capture).
    Draining,
    /// Paused.
    Paused,
    /// Hardware is suspended.
    Suspended,
    /// Hardware is disconnected.
    Disconnected,
}

impl Device {
    /// Opens a PCM.
    ///
    /// # Panics
    ///
    /// If `name` contains null bytes.
    pub fn open<T: AsRef<str>>(name: T) -> AlsaResult<Self> {
        let mut snd_pcm = ptr::null_mut();
        let name = CString::new(name.as_ref()).unwrap();
        unsafe {
            snd_pcm_open(&mut snd_pcm, name.as_ptr(), SND_PCM_STREAM_PLAYBACK, 0)
                .to_alsa_result()?;
        }
        Ok(Self { snd_pcm })
    }

    /// Installs one PCM hardware configuration chosen from a configuration
    /// space and [`prepare`](Device::prepare) it.
    pub fn hw_params(&mut self, hw_params: &mut HwParams) -> AlsaResult<()> {
        unsafe {
            snd_pcm_hw_params(self.as_raw(), hw_params.as_raw()).to_alsa_result()?;
        };
        Ok(())
    }

    /// Returns PCM state.
    pub fn state(&mut self) -> State {
        unsafe { snd_pcm_state(self.as_raw()).into() }
    }

    /// Prepares PCM for use.
    pub fn prepare(&mut self) -> AlsaResult<()> {
        unsafe { snd_pcm_prepare(self.as_raw()).to_alsa_result() }
    }

    /// Resets PCM position.
    pub fn reset(&mut self) -> AlsaResult<()> {
        unsafe { snd_pcm_reset(self.as_raw()).to_alsa_result() }
    }

    /// Starts the PCM.
    pub fn start(&mut self) -> AlsaResult<()> {
        unsafe { snd_pcm_start(self.as_raw()).to_alsa_result() }
    }

    /// Stops the PCM dropping pending frames.
    pub fn drop(&mut self) -> AlsaResult<()> {
        unsafe { snd_pcm_drop(self.as_raw()).to_alsa_result() }
    }

    /// Stops the PCM preserving pending frames.
    pub fn drain(&mut self) -> AlsaResult<()> {
        unsafe { snd_pcm_drain(self.as_raw()).to_alsa_result() }
    }

    /// Pauses/resumes the PCM.
    pub fn pause(&mut self, enable: bool) -> AlsaResult<()> {
        unsafe { snd_pcm_pause(self.as_raw(), enable.into()).to_alsa_result() }
    }

    /// Resumes from suspend, no samples are lost.
    pub fn resume(&mut self) -> AlsaResult<()> {
        unsafe { snd_pcm_resume(self.as_raw()).to_alsa_result() }
    }

    /// Writes a WAV file from a generic `reader` to the PCM buffer. Returns
    /// the duration of the sound.
    #[allow(clippy::similar_names)] // complains about `reader` and `header`
    pub fn play_wav<T: Read + Seek>(
        &mut self,
        reader: &mut T,
        hw_params: &mut HwParams,
        volume: f64,
    ) -> io::Result<Duration> {
        let wav = riff::Chunk::read(reader, 0)?;
        if wav.read_type(reader)?.as_str() != "WAVE" {
            return Err(io::Error::other("RIFF file type is not WAVE"));
        }

        let header = wav
            .iter(reader)
            .filter_map(std::result::Result::ok)
            .find(|chunk| chunk.id().as_str() == "fmt ")
            .map(|chunk| chunk.read_contents(reader))
            .transpose()?
            .ok_or_else(|| {
                io::Error::other("RIFF data is missing the \"fmt \" chunk")
            })?;

        let audio_format = u16::from_le_bytes([header[0], header[1]]);
        let channel_count = u16::from_le_bytes([header[2], header[3]]);
        let sampling_rate =
            u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let bits_per_sample = u16::from_le_bytes([header[14], header[15]]);

        if audio_format != WAV_FORMAT_PCM && audio_format != WAV_FORMAT_EXTENSIBLE {
            return Err(io::Error::other("WAV is not in PCM format"));
        }
        hw_params.any(self).map_err(alsa_to_io_error)?;
        hw_params
            .set_access(self, Access::RwInterleaved)
            .map_err(alsa_to_io_error)?;
        hw_params
            .set_channels(self, channel_count.into())
            .map_err(alsa_to_io_error)?;
        hw_params
            .set_rate(self, sampling_rate)
            .map_err(alsa_to_io_error)?;
        let format = match bits_per_sample {
            16 => Format::S16Le,
            32 => Format::S32Le,
            bits_per_sample => {
                return Err(io::Error::other(format!(
                    "Unsupported bits_per_sample value {bits_per_sample}"
                )));
            }
        };
        hw_params
            .set_format(self, format)
            .map_err(alsa_to_io_error)?;

        while let Err(err) = self.hw_params(hw_params).map_err(alsa_to_io_error) {
            const RETRY_TIMEOUT_US: u64 = 1000;
            tracing::error!(
                "Error setting hw_params: {}. Retrying in {} us...",
                err,
                RETRY_TIMEOUT_US
            );
            sleep(Duration::from_millis(RETRY_TIMEOUT_US));
        }

        let (offset, len) = wav
            .iter(reader)
            .filter_map(std::result::Result::ok)
            .find(|chunk| chunk.id().as_str() == "data")
            .map(|chunk| (chunk.offset(), chunk.len()))
            .ok_or_else(|| {
                io::Error::other("RIFF data is missing the \"data\" chunk")
            })?;

        reader.seek(io::SeekFrom::Start(offset + 8))?;
        let reader = reader.take(len.into());
        match format {
            Format::S16Le => {
                i16::volume_adjusted_copy(reader, self, volume)?;
                Ok(Duration::from_secs_f64(
                    f64::from(len)
                        / (2.0 * f64::from(sampling_rate) * f64::from(channel_count)),
                ))
            }
            Format::S32Le => {
                i32::volume_adjusted_copy(reader, self, volume)?;
                Ok(Duration::from_secs_f64(
                    f64::from(len)
                        / (4.0 * f64::from(sampling_rate) * f64::from(channel_count)),
                ))
            }
            _ => panic!("unsupported format {format:?}"),
        }
    }

    pub(crate) fn as_raw(&mut self) -> *mut snd_pcm_t {
        self.snd_pcm
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            snd_pcm_close(self.as_raw())
                .to_alsa_result()
                .expect("couldn't close ALSA device");
        }
    }
}

impl Write for Device {
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap
    )]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            unsafe {
                let frames = snd_pcm_bytes_to_frames(self.as_raw(), buf.len() as _);
                let written =
                    snd_pcm_writei(self.as_raw(), buf.as_ptr().cast(), frames as _);
                if written == -i64::from(EPIPE)
                    || written == -i64::from(128 /* ESTRPIPE */)
                {
                    tracing::error!("audio buffer underrun occurred");
                    snd_pcm_recover(self.as_raw(), written as _, 0)
                        .to_alsa_result()
                        .map_err(alsa_to_io_error)?;
                    continue;
                }
                written.to_alsa_result().map_err(alsa_to_io_error)?;
                break Ok(snd_pcm_frames_to_bytes(self.as_raw(), written) as _);
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl From<snd_pcm_state_t> for State {
    fn from(state: snd_pcm_state_t) -> Self {
        match state {
            state if state == SND_PCM_STATE_OPEN => Self::Open,
            state if state == SND_PCM_STATE_SETUP => Self::Setup,
            state if state == SND_PCM_STATE_PREPARED => Self::Prepared,
            state if state == SND_PCM_STATE_RUNNING => Self::Running,
            state if state == SND_PCM_STATE_XRUN => Self::Xrun,
            state if state == SND_PCM_STATE_DRAINING => Self::Draining,
            state if state == SND_PCM_STATE_PAUSED => Self::Paused,
            state if state == SND_PCM_STATE_SUSPENDED => Self::Suspended,
            state if state == SND_PCM_STATE_DISCONNECTED => Self::Disconnected,
            _ => panic!("invalid snd_pcm_state_t value"),
        }
    }
}

trait Sample
where
    Self: Sized + Copy,
    f64: From<Self>,
{
    #[allow(clippy::cast_ptr_alignment)]
    fn volume_adjusted_copy<R: io::Read, W: io::Write>(
        reader: R,
        writer: &mut W,
        volume: f64,
    ) -> io::Result<()> {
        let mut reader = io::BufReader::new(reader);
        loop {
            let mut buf = reader.fill_buf()?.to_vec();
            if buf.is_empty() {
                break;
            }
            let samples_ptr = buf.as_mut_ptr().cast::<Self>();
            let samples_len = buf.len() / 2;
            for i in 0..samples_len {
                unsafe {
                    let x = samples_ptr.add(i);
                    *x = Self::from_f64(f64::from(*x) * volume);
                }
            }
            let bytes_len = samples_len * 2;
            writer.write_all(&buf[..bytes_len])?;
            reader.consume(bytes_len);
        }
        Ok(())
    }

    fn from_f64(value: f64) -> Self;
}

impl Sample for i16 {
    #[allow(clippy::cast_possible_truncation)]
    fn from_f64(value: f64) -> Self {
        value as _
    }
}

impl Sample for i32 {
    #[allow(clippy::cast_possible_truncation)]
    fn from_f64(value: f64) -> Self {
        value as _
    }
}
