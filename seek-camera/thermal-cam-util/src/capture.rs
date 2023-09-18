use clap::{Args, Subcommand};
use color_eyre::{eyre::bail, eyre::WrapErr, Help, Result};
use indicatif::ProgressIterator;
use owo_colors::OwoColorize;
use seek_camera::{
    filters::{Filter, FilterState},
    frame::FrameContainer,
    frame_format::FrameFormat,
    manager::{CameraHandle, Manager},
};
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::mpsc::SyncSender,
};

use crate::{start_manager, Flow};

#[derive(Debug, Args)]
pub struct Capture {
    #[clap(subcommand)]
    commands: Commands,
}

impl Capture {
    pub fn run(self) -> Result<()> {
        match self.commands {
            Commands::Save(c) => c.run(),
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    Save(Save),
}

#[derive(Debug, Args)]
pub struct Save {
    /// Warmup time in seconds before starting the flat scene calibration.
    save_dir: PathBuf,
    /// The number of frames to save before terminating
    #[arg(default_value_t = 1)]
    num_frames: u16,
    /// Disables the Flat Scene Calibration filter.
    #[arg(long)]
    no_fsc: bool,
}

impl Save {
    pub fn run(self) -> Result<()> {
        if !self.save_dir.is_dir() {
            bail!("Please provide a valid directory that exists for `save_dir`");
        }
        if self.save_dir.read_dir()?.next().is_some() {
            eprintln!("{}", "Warning: `save_dir` is not empty".yellow());
        }
        start_manager(Box::new(move |mngr, cam_h, _evt, _err| {
            on_cam(mngr, cam_h, self.num_frames, &self.save_dir, self.no_fsc)
        }))
    }
}

fn on_cam(
    mngr: &mut Manager,
    cam_h: CameraHandle,
    num_frames: u16,
    save_dir: &Path,
    no_fsc: bool,
) -> Result<Flow> {
    mngr.camera_mut(cam_h, |cam| -> Result<Flow> {
        let cam = cam.unwrap();
        if !cam.is_paired() {
            bail!("Camera must be paired before saving frames");
        }

        // Setup the callback
        let (tx, rx) = std::sync::mpsc::sync_channel(num_frames as usize);
        cam.set_callback(Box::new(move |frame_container| {
            on_frame(frame_container, &tx).expect("Failed inside frame event handler");
        }))?;

        cam.capture_session_start(FrameFormat::Grayscale)
            .wrap_err("Failed to start capture session")?;
        // Oddly, this appears to have no effect unless run *after* the session is
        // started.
        if no_fsc {
            cam.set_filter_state(Filter::FlatSceneCorrection, FilterState::Disabled)
                .wrap_err("Failed to disable FSC")?;
        }

        // Collect the frame data
        for i in (0..num_frames).progress() {
            let png_path = save_dir.join(format!("{i}.png"));
            let png_file = File::create(png_path)
                .wrap_err("Failed to create png file")
                .with_suggestion(|| format!("Does {} exist?", save_dir.display()))?;
            let mut writer = BufWriter::new(png_file);
            let data = rx.recv().wrap_err("Failed to receive png data")?;
            writer
                .write_all(&data)
                .wrap_err("Failed to write bytes into file")?;
        }
        cam.capture_session_stop()?;
        Ok(Flow::Finish)
    })
}

fn on_frame(fc: FrameContainer, tx: &SyncSender<Vec<u8>>) -> Result<()> {
    let frame = fc
        .get_frame::<seek_camera::frame_format::GrayscalePixel>()
        .wrap_err("Failed to extract frame format")?;
    let mut buf = Vec::with_capacity(frame.width() * frame.height());
    let cursor = std::io::Cursor::new(&mut buf);

    let mut encoder =
        png::Encoder::new(cursor, frame.width() as _, frame.height() as _);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);

    let mut writer = encoder
        .write_header()
        .wrap_err("failed to write png header")?;
    writer
        .write_image_data(bytemuck::cast_slice(frame.pixels()))
        .wrap_err("failed to encode png data")?;
    writer
        .finish()
        .wrap_err("Failed to finish writing png data")?;

    // Failure to send is expected if the buffer fills up or the program terminates
    let _ = tx.send(buf);
    Ok(())
}
