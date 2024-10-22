use super::{AlsaResult, Device, ToAlsaResult};
use alsa_sys::{
    snd_pcm_access_t, snd_pcm_format_t, snd_pcm_hw_params_any, snd_pcm_hw_params_free,
    snd_pcm_hw_params_malloc, snd_pcm_hw_params_set_access,
    snd_pcm_hw_params_set_channels, snd_pcm_hw_params_set_format,
    snd_pcm_hw_params_set_rate, snd_pcm_hw_params_set_rate_resample,
    snd_pcm_hw_params_t, SND_PCM_ACCESS_MMAP_COMPLEX, SND_PCM_ACCESS_MMAP_INTERLEAVED,
    SND_PCM_ACCESS_MMAP_NONINTERLEAVED, SND_PCM_ACCESS_RW_INTERLEAVED,
    SND_PCM_ACCESS_RW_NONINTERLEAVED, SND_PCM_FORMAT_A_LAW, SND_PCM_FORMAT_FLOAT,
    SND_PCM_FORMAT_FLOAT64, SND_PCM_FORMAT_FLOAT64_BE, SND_PCM_FORMAT_FLOAT64_LE,
    SND_PCM_FORMAT_FLOAT_BE, SND_PCM_FORMAT_FLOAT_LE, SND_PCM_FORMAT_GSM,
    SND_PCM_FORMAT_IEC958_SUBFRAME, SND_PCM_FORMAT_IEC958_SUBFRAME_BE,
    SND_PCM_FORMAT_IEC958_SUBFRAME_LE, SND_PCM_FORMAT_IMA_ADPCM, SND_PCM_FORMAT_MPEG,
    SND_PCM_FORMAT_MU_LAW, SND_PCM_FORMAT_S16, SND_PCM_FORMAT_S16_BE,
    SND_PCM_FORMAT_S16_LE, SND_PCM_FORMAT_S18_3BE, SND_PCM_FORMAT_S18_3LE,
    SND_PCM_FORMAT_S20_3BE, SND_PCM_FORMAT_S20_3LE, SND_PCM_FORMAT_S24,
    SND_PCM_FORMAT_S24_3BE, SND_PCM_FORMAT_S24_3LE, SND_PCM_FORMAT_S24_BE,
    SND_PCM_FORMAT_S24_LE, SND_PCM_FORMAT_S32, SND_PCM_FORMAT_S32_BE,
    SND_PCM_FORMAT_S32_LE, SND_PCM_FORMAT_S8, SND_PCM_FORMAT_SPECIAL,
    SND_PCM_FORMAT_U16, SND_PCM_FORMAT_U16_BE, SND_PCM_FORMAT_U16_LE,
    SND_PCM_FORMAT_U18_3BE, SND_PCM_FORMAT_U18_3LE, SND_PCM_FORMAT_U20_3BE,
    SND_PCM_FORMAT_U20_3LE, SND_PCM_FORMAT_U24, SND_PCM_FORMAT_U24_3BE,
    SND_PCM_FORMAT_U24_3LE, SND_PCM_FORMAT_U24_BE, SND_PCM_FORMAT_U24_LE,
    SND_PCM_FORMAT_U32, SND_PCM_FORMAT_U32_BE, SND_PCM_FORMAT_U32_LE,
    SND_PCM_FORMAT_U8, SND_PCM_FORMAT_UNKNOWN,
};
use std::ptr;

/// PCM hardware configuration space container.
pub struct HwParams {
    hw_params: *mut snd_pcm_hw_params_t,
}

unsafe impl Send for HwParams {}

/// PCM access type.
#[derive(Clone, Copy, Debug)]
pub enum Access {
    /// MMAP access with simple interleaved channels.
    MmapInterleaved,
    /// MMAP access with simple non interleaved channels.
    MmapNoninterleaved,
    /// MMAP access with complex placement.
    MmapComplex,
    /// `snd_pcm_readi`/`snd_pcm_writei` access.
    RwInterleaved,
    /// `snd_pcm_readn`/`snd_pcm_writen` access.
    RwNoninterleaved,
}

/// PCM sample format.
#[derive(Clone, Copy, Debug)]
pub enum Format {
    /// Unknown.
    Unknown,
    /// Signed 8 bit.
    S8,
    /// Unsigned 8 bit.
    U8,
    /// Signed 16 bit Little Endian.
    S16Le,
    /// Signed 16 bit Big Endian.
    S16Be,
    /// Unsigned 16 bit Little Endian.
    U16Le,
    /// Unsigned 16 bit Big Endian.
    U16Be,
    /// Signed 24 bit Little Endian using low three bytes in 32-bit word.
    S24Le,
    /// Signed 24 bit Big Endian using low three bytes in 32-bit word.
    S24Be,
    /// Unsigned 24 bit Little Endian using low three bytes in 32-bit word.
    U24Le,
    /// Unsigned 24 bit Big Endian using low three bytes in 32-bit word.
    U24Be,
    /// Signed 32 bit Little Endian.
    S32Le,
    /// Signed 32 bit Big Endian.
    S32Be,
    /// Unsigned 32 bit Little Endian.
    U32Le,
    /// Unsigned 32 bit Big Endian.
    U32Be,
    /// Float 32 bit Little Endian, Range -1.0 to 1.0.
    FloatLe,
    /// Float 32 bit Big Endian, Range -1.0 to 1.0.
    FloatBe,
    /// Float 64 bit Little Endian, Range -1.0 to 1.0.
    FloaT64Le,
    /// Float 64 bit Big Endian, Range -1.0 to 1.0.
    FloaT64Be,
    /// IEC-958 Little Endian.
    IeC958SubframeLe,
    /// IEC-958 Big Endian.
    IeC958SubframeBe,
    /// Mu-Law.
    MuLaw,
    /// A-Law.
    ALaw,
    /// Ima-ADPCM.
    ImaAdpcm,
    /// MPEG.
    Mpeg,
    /// GSM.
    Gsm,
    /// Special.
    Special,
    /// Signed 24bit Little Endian in 3bytes format.
    S243Le,
    /// Signed 24bit Big Endian in 3bytes format.
    S243Be,
    /// Unsigned 24bit Little Endian in 3bytes format.
    U243Le,
    /// Unsigned 24bit Big Endian in 3bytes format.
    U243Be,
    /// Signed 20bit Little Endian in 3bytes format.
    S203Le,
    /// Signed 20bit Big Endian in 3bytes format.
    S203Be,
    /// Unsigned 20bit Little Endian in 3bytes format.
    U203Le,
    /// Unsigned 20bit Big Endian in 3bytes format.
    U203Be,
    /// Signed 18bit Little Endian in 3bytes format.
    S183Le,
    /// Signed 18bit Big Endian in 3bytes format.
    S183Be,
    /// Unsigned 18bit Little Endian in 3bytes format.
    U183Le,
    /// Unsigned 18bit Big Endian in 3bytes format.
    U183Be,
    /// Signed 16 bit CPU endian.
    S16,
    /// Unsigned 16 bit CPU endian.
    U16,
    /// Signed 24 bit CPU endian.
    S24,
    /// Unsigned 24 bit CPU endian.
    U24,
    /// Signed 32 bit CPU endian.
    S32,
    /// Unsigned 32 bit CPU endian.
    U32,
    /// Float 32 bit CPU endian.
    Float,
    /// Float 64 bit CPU endian.
    FloaT64,
    /// IEC-958 CPU Endian.
    IeC958Subframe,
}

impl HwParams {
    /// Allocates an invalid `HwParams` using standard `malloc`.
    pub fn new() -> AlsaResult<Self> {
        let mut hw_params = ptr::null_mut();
        unsafe { snd_pcm_hw_params_malloc(&mut hw_params).to_alsa_result()? };
        Ok(Self { hw_params })
    }

    /// Fills params with a full configuration space for a PCM.
    pub fn any(&mut self, device: &mut Device) -> AlsaResult<()> {
        unsafe {
            snd_pcm_hw_params_any(device.as_raw(), self.as_raw()).to_alsa_result()?;
        };
        Ok(())
    }

    /// Restricts a configuration space to contain only real hardware rates.
    pub fn set_rate_resample(
        &mut self,
        device: &mut Device,
        resample: bool,
    ) -> AlsaResult<()> {
        unsafe {
            snd_pcm_hw_params_set_rate_resample(
                device.as_raw(),
                self.as_raw(),
                resample.into(),
            )
            .to_alsa_result()?;
        }
        Ok(())
    }

    /// Restricts a configuration space to contain only one access type.
    pub fn set_access(
        &mut self,
        device: &mut Device,
        access: Access,
    ) -> AlsaResult<()> {
        unsafe {
            snd_pcm_hw_params_set_access(device.as_raw(), self.as_raw(), access.into())
                .to_alsa_result()?;
        }
        Ok(())
    }

    /// Restricts a configuration space to contain only one format.
    pub fn set_format(
        &mut self,
        device: &mut Device,
        format: Format,
    ) -> AlsaResult<()> {
        unsafe {
            snd_pcm_hw_params_set_format(device.as_raw(), self.as_raw(), format.into())
                .to_alsa_result()?;
        }
        Ok(())
    }

    /// Restricts a configuration space to contain only one channels count.
    pub fn set_channels(
        &mut self,
        device: &mut Device,
        channels: u32,
    ) -> AlsaResult<()> {
        unsafe {
            snd_pcm_hw_params_set_channels(device.as_raw(), self.as_raw(), channels)
                .to_alsa_result()?;
        }
        Ok(())
    }

    /// Restricts a configuration space to contain only one rate.
    pub fn set_rate(&mut self, device: &mut Device, rate: u32) -> AlsaResult<()> {
        unsafe {
            snd_pcm_hw_params_set_rate(device.as_raw(), self.as_raw(), rate, 0)
                .to_alsa_result()?;
        }
        Ok(())
    }

    pub(crate) fn as_raw(&mut self) -> *mut snd_pcm_hw_params_t {
        self.hw_params
    }
}

impl Drop for HwParams {
    fn drop(&mut self) {
        unsafe { snd_pcm_hw_params_free(self.as_raw()) };
    }
}

#[allow(clippy::from_over_into)]
impl Into<snd_pcm_access_t> for Access {
    fn into(self) -> snd_pcm_access_t {
        match self {
            Self::MmapInterleaved => SND_PCM_ACCESS_MMAP_INTERLEAVED,
            Self::MmapNoninterleaved => SND_PCM_ACCESS_MMAP_NONINTERLEAVED,
            Self::MmapComplex => SND_PCM_ACCESS_MMAP_COMPLEX,
            Self::RwInterleaved => SND_PCM_ACCESS_RW_INTERLEAVED,
            Self::RwNoninterleaved => SND_PCM_ACCESS_RW_NONINTERLEAVED,
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<snd_pcm_format_t> for Format {
    fn into(self) -> snd_pcm_format_t {
        match self {
            Self::Unknown => SND_PCM_FORMAT_UNKNOWN,
            Self::S8 => SND_PCM_FORMAT_S8,
            Self::U8 => SND_PCM_FORMAT_U8,
            Self::S16Le => SND_PCM_FORMAT_S16_LE,
            Self::S16Be => SND_PCM_FORMAT_S16_BE,
            Self::U16Le => SND_PCM_FORMAT_U16_LE,
            Self::U16Be => SND_PCM_FORMAT_U16_BE,
            Self::S24Le => SND_PCM_FORMAT_S24_LE,
            Self::S24Be => SND_PCM_FORMAT_S24_BE,
            Self::U24Le => SND_PCM_FORMAT_U24_LE,
            Self::U24Be => SND_PCM_FORMAT_U24_BE,
            Self::S32Le => SND_PCM_FORMAT_S32_LE,
            Self::S32Be => SND_PCM_FORMAT_S32_BE,
            Self::U32Le => SND_PCM_FORMAT_U32_LE,
            Self::U32Be => SND_PCM_FORMAT_U32_BE,
            Self::FloatLe => SND_PCM_FORMAT_FLOAT_LE,
            Self::FloatBe => SND_PCM_FORMAT_FLOAT_BE,
            Self::FloaT64Le => SND_PCM_FORMAT_FLOAT64_LE,
            Self::FloaT64Be => SND_PCM_FORMAT_FLOAT64_BE,
            Self::IeC958SubframeLe => SND_PCM_FORMAT_IEC958_SUBFRAME_LE,
            Self::IeC958SubframeBe => SND_PCM_FORMAT_IEC958_SUBFRAME_BE,
            Self::MuLaw => SND_PCM_FORMAT_MU_LAW,
            Self::ALaw => SND_PCM_FORMAT_A_LAW,
            Self::ImaAdpcm => SND_PCM_FORMAT_IMA_ADPCM,
            Self::Mpeg => SND_PCM_FORMAT_MPEG,
            Self::Gsm => SND_PCM_FORMAT_GSM,
            Self::Special => SND_PCM_FORMAT_SPECIAL,
            Self::S243Le => SND_PCM_FORMAT_S24_3LE,
            Self::S243Be => SND_PCM_FORMAT_S24_3BE,
            Self::U243Le => SND_PCM_FORMAT_U24_3LE,
            Self::U243Be => SND_PCM_FORMAT_U24_3BE,
            Self::S203Le => SND_PCM_FORMAT_S20_3LE,
            Self::S203Be => SND_PCM_FORMAT_S20_3BE,
            Self::U203Le => SND_PCM_FORMAT_U20_3LE,
            Self::U203Be => SND_PCM_FORMAT_U20_3BE,
            Self::S183Le => SND_PCM_FORMAT_S18_3LE,
            Self::S183Be => SND_PCM_FORMAT_S18_3BE,
            Self::U183Le => SND_PCM_FORMAT_U18_3LE,
            Self::U183Be => SND_PCM_FORMAT_U18_3BE,
            Self::S16 => SND_PCM_FORMAT_S16,
            Self::U16 => SND_PCM_FORMAT_U16,
            Self::S24 => SND_PCM_FORMAT_S24,
            Self::U24 => SND_PCM_FORMAT_U24,
            Self::S32 => SND_PCM_FORMAT_S32,
            Self::U32 => SND_PCM_FORMAT_U32,
            Self::Float => SND_PCM_FORMAT_FLOAT,
            Self::FloaT64 => SND_PCM_FORMAT_FLOAT64,
            Self::IeC958Subframe => SND_PCM_FORMAT_IEC958_SUBFRAME,
        }
    }
}
