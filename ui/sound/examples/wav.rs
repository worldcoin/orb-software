use color_eyre::eyre::Result;
use orb_sound::{Device, HwParams};
use std::fs::File;

fn main() -> Result<()> {
    let mut device = Device::open("default")?;
    let mut hw_params = HwParams::new()?;

    let mut wav = File::open("sound/assets/voice_connected.wav")?;
    device.play_wav(&mut wav, &mut hw_params, 1.0)?;
    device.drain()?;

    Ok(())
}
