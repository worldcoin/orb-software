use crate::sys::frame_format_t;

use bytemuck::{Pod, Zeroable};
use fixed::{types::extra::U6, FixedU16};

mod private {
    /// Internal-only type that prevents implementing traits
    pub trait Sealed {}
}

/// All possible frame formats to support.
#[repr(u32)]
pub enum FrameFormat {
    /// U10.6 fixed point decimal in degrees celcius. See also [`ThermographyFixedPixel`].
    ThermographyFixed = frame_format_t::ThermographyFixed106.0,
    /// Alpha, Red, Green, Blue, 1 byte each. See also [`ColorArgb8888Pixel`].
    ColorArgb8888 = frame_format_t::ColorArgb8888.0,
    /// A single byte. See also [`GrayscalePixel`].
    Grayscale = frame_format_t::Grayscale.0,
}

/// All pixel types implement this trait.
pub trait Pixel: Pod + private::Sealed {
    /// Number of bits in a pixel.
    const PIXEL_DEPTH: u8;
    /// Number of channels in a pixel.
    const CHANNELS: u8;
    const FRAME_FORMAT: FrameFormat;
}

/// Pixels for the thermography formats support directly converting to degrees celcius.
pub trait AsCelcius: private::Sealed {
    fn as_celcius_f32(&self) -> f32;
}

//-------------------------//
// ---- Frame formats ---- //
//-------------------------//

/// Pixel for [`FrameFormat::ThermographyFixed`].
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Eq, PartialEq, Debug)]
pub struct ThermographyFixedPixel(pub FixedU16<U6>);

impl private::Sealed for ThermographyFixedPixel {}

impl Pixel for ThermographyFixedPixel {
    const CHANNELS: u8 = 1;
    const FRAME_FORMAT: FrameFormat = FrameFormat::ThermographyFixed;
    const PIXEL_DEPTH: u8 = 16;
}

impl AsCelcius for ThermographyFixedPixel {
    fn as_celcius_f32(&self) -> f32 {
        self.0.to_num::<f32>() - 40. // See page 54 of seek thermal C sdk
    }
}

/// Pixel for [`FrameFormat::ColorArgb8888`]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug, Eq, PartialEq)]
pub struct ColorArgb8888Pixel {
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl private::Sealed for ColorArgb8888Pixel {}

impl Pixel for ColorArgb8888Pixel {
    const CHANNELS: u8 = 4;
    const FRAME_FORMAT: FrameFormat = FrameFormat::ColorArgb8888;
    const PIXEL_DEPTH: u8 = 32;
}

/// Pixel for [`FrameFormat::Grayscale`]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug, Eq, PartialEq)]
pub struct GrayscalePixel(pub u8);

impl private::Sealed for GrayscalePixel {}

impl Pixel for GrayscalePixel {
    const CHANNELS: u8 = 1;
    const FRAME_FORMAT: FrameFormat = FrameFormat::Grayscale;
    const PIXEL_DEPTH: u8 = 8;
}
