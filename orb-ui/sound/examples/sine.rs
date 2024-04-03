use color_eyre::eyre::Result;
use orb_sound::{Access, Device, Format, HwParams};
use std::{f32::consts::PI, io::prelude::*};

const SINE_LENGTH: u32 = 1024;
const RATE: u32 = 44100;

fn main() -> Result<()> {
    let mut device = Device::open("default")?;
    let mut hw_params = HwParams::new()?;
    hw_params.any(&mut device)?;
    hw_params.set_rate_resample(&mut device, false)?;
    hw_params.set_access(&mut device, Access::RwInterleaved)?;
    hw_params.set_format(&mut device, Format::S16Le)?;
    hw_params.set_channels(&mut device, 2)?;
    hw_params.set_rate(&mut device, RATE)?;
    device.hw_params(&mut hw_params)?;

    // Make a sine wave
    let mut buf = [0; SINE_LENGTH as usize * 2];
    for i in 0..SINE_LENGTH as usize {
        unsafe {
            *buf.as_mut_ptr().cast::<i16>().add(i) =
                ((i as f32 * 2.0 * PI / 512.0).sin() * 8192.0) as _;
        }
    }

    // Play it back for 2 seconds.
    for _ in 0..2 * RATE / SINE_LENGTH {
        device.write_all(&buf)?;
    }

    device.drain()?;

    Ok(())
}
