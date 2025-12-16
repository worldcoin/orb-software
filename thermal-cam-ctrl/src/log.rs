#![allow(clippy::uninlined_format_args)]
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

use clap::Args;
use color_eyre::{
    eyre::{eyre, WrapErr},
    Result,
};
use owo_colors::OwoColorize;
use seek_camera::{
    manager::{CameraHandle, Event, Manager},
    ErrorCode,
};

use crate::{get_seek_dir, start_manager, Flow};

/// Logs events from the camera
///
/// The SDK log file will be logged to stderr, and the logs from interacting
/// with the SDK directly will be logged to stdout.
#[derive(Debug, Args)]
pub struct Log {
    /// Don't print the log file that the SDK saves to the filesystem.
    #[clap(long)]
    no_fs_log: bool,
    /// Don't print the info we get from the api.
    #[clap(long)]
    no_api_log: bool,
}
impl Log {
    pub fn run(self) -> Result<()> {
        let path = get_seek_dir().join("log").join("seekcamera.log");
        let parent = path.parent();
        if let Some(parent) = parent {
            std::fs::create_dir_all(parent)
                .wrap_err("failed to create seek log dir")?;
        }

        let fs_handle = if !self.no_fs_log {
            if !path.exists() {
                File::options()
                    .create_new(true)
                    .append(true)
                    .open(&path)
                    .wrap_err("Failed to create empty sdk file")?;
            }
            let mut logfile =
                File::open(&path).wrap_err("Failed to open sdk log file")?;
            logfile
                .seek(SeekFrom::End(0))
                .wrap_err("Failed to seek to end of sdk log file")?;

            // Streams from log file to stderr
            let fs_handle = std::thread::spawn(move || -> Result<()> {
                let mut stderr = std::io::BufWriter::new(std::io::stderr());
                let mut buf = [0; 256];
                loop {
                    let nb =
                        logfile.read(&mut buf).expect("Failed to read from logfile");
                    stderr.write_all(&buf[..nb]).unwrap();
                    stderr.flush().unwrap();
                }
            });
            Some(fs_handle)
        } else {
            None
        };

        let api_handle = if !self.no_api_log {
            Some(std::thread::spawn(|| start_manager(Box::new(on_cam_event))))
        } else {
            None
        };

        if let Some(h) = fs_handle
            && let Err(err) = h.join().unwrap()
        {
            eprintln!("{:?}", err);
        }
        if let Some(h) = api_handle
            && let Err(err) = h.join().unwrap()
        {
            println!("{:?}", err);
        }

        Ok(())
    }
}

fn on_cam_event(
    mngr: &mut Manager,
    cam_h: CameraHandle,
    evt: Event,
    err: Option<ErrorCode>,
) -> Result<Flow> {
    let cid = mngr
        .cameras()
        .unwrap()
        .get_mut(&cam_h)
        .ok_or_else(|| eyre!("Could not get camera from handle"))?
        .chip_id()
        .wrap_err("Failed to get camera chip id")?;
    let str = format!("Camera({cid}) - event: {evt:?}, err: {err:?}");
    println!("{}", str.green());
    Ok(Flow::Continue)
}
