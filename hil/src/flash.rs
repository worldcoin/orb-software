use std::path::Path;

use camino::Utf8Path;
use cmd_lib::run_cmd;
use color_eyre::{
    eyre::{ensure, WrapErr},
    Result, Section,
};
use tempfile::TempDir;

pub async fn flash(variant: FlashVariant, path_to_rts_tar: &Utf8Path) -> Result<()> {
    let path_to_rts = path_to_rts_tar.to_owned();
    tokio::task::spawn_blocking(move || {
        let tmp_dir = extract(&path_to_rts)?;
        println!("{tmp_dir:?}");
        flash_cmd(variant, tmp_dir.path())?;
        Ok(())
    })
    .await
    .wrap_err("task panicked")?
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FlashVariant {
    Fast,
    Regular,
}

impl FlashVariant {
    fn file_name(&self) -> &'static str {
        match self {
            FlashVariant::Fast => "fastflashcmd.txt",
            FlashVariant::Regular => "flashcmd.txt",
        }
    }
}

fn extract(path_to_rts: &Utf8Path) -> Result<TempDir> {
    ensure!(
        path_to_rts.try_exists().unwrap_or(false),
        "{path_to_rts} doesn't exist"
    );
    ensure!(path_to_rts.is_file(), "{path_to_rts} should be a file!");
    let temp_dir = TempDir::new_in(path_to_rts.parent().unwrap())
        .wrap_err("failed to create temporary extract dir")?;
    let extract_dir = temp_dir.path();
    let result = run_cmd! {
        cd $extract_dir;
        info extracting rts $path_to_rts;
        tar xvf $path_to_rts;
        info finished extract!;
    };
    result
        .wrap_err("failed to extract rts")
        .with_note(|| format!("path_to_rts was {path_to_rts}"))?;
    Ok(temp_dir)
}

fn flash_cmd(variant: FlashVariant, extracted_dir: &Path) -> Result<()> {
    let bootloader_dir = extracted_dir.join("ready-to-sign").join("bootloader");
    ensure!(
        bootloader_dir.try_exists().unwrap_or(false),
        "{bootloader_dir:?} doesn't exist"
    );

    let cmd_file_name = variant.file_name();
    let result = run_cmd! {
        cd $bootloader_dir;
        info running $cmd_file_name;
        bash $cmd_file_name;
        info finished flashing!;
    };
    result
        .wrap_err("failed to flash rts")
        .with_note(|| format!("bootloader_dir was {bootloader_dir:?}"))?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
}
