#![allow(clippy::uninlined_format_args)]
use std::io::Write;
use std::path::Path;
use std::{fs::File, path::PathBuf};

use camino::Utf8Path;
use cmd_lib::run_cmd;
use color_eyre::{
    eyre::{bail, ensure, WrapErr},
    Result, Section,
};
use tempfile::TempDir;

use crate::boot::is_recovery_mode_detected;

pub async fn flash(
    variant: FlashVariant,
    path_to_rts_tar: &Utf8Path,
    persistent_img_path: &Path,
    rng: (impl rand::Rng + Send + 'static),
) -> Result<()> {
    ensure!(
        is_recovery_mode_detected().await?,
        "orb not in recovery mode"
    );

    let path_to_rts = path_to_rts_tar.to_owned();
    let persistent_img_path = persistent_img_path.to_owned();

    let tmp_dir = tokio::task::spawn_blocking(move || extract(&path_to_rts))
        .await
        .wrap_err("task panicked")??;
    println!("{tmp_dir:?}");

    let tmp_dir_path = tmp_dir.path().to_path_buf();
    populate_persistent(tmp_dir_path, persistent_img_path, rng).await?;

    ensure!(
        is_recovery_mode_detected().await?,
        "orb not in recovery mode"
    );
    tokio::task::spawn_blocking(move || flash_cmd(variant, tmp_dir.path()))
        .await
        .wrap_err("task panicked")??;

    Ok(())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FlashVariant {
    Fast,
    Regular,
    HilFast,
    Hil,
    Nfsboot,
}

impl FlashVariant {
    fn file_name(&self) -> &'static str {
        match self {
            FlashVariant::Fast => "fastflashcmd.txt",
            FlashVariant::Regular => "flashcmd.txt",
            FlashVariant::HilFast => "hil-fastflashcmd.txt",
            FlashVariant::Hil => "hil-flashcmd.txt",
            FlashVariant::Nfsboot => "nfsbootcmd.sh",
        }
    }
}

pub(crate) fn extract(path_to_rts: &Utf8Path) -> Result<TempDir> {
    ensure!(
        path_to_rts.try_exists().unwrap_or(false),
        "{path_to_rts} doesn't exist"
    );
    ensure!(path_to_rts.is_file(), "{path_to_rts} should be a file!");
    let path_to_rts = path_to_rts
        .canonicalize()
        .wrap_err_with(|| format!("failed to canonicalize path: {}", path_to_rts))?;
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
        .with_note(|| format!("path_to_rts was {}", path_to_rts.display()))?;

    Ok(temp_dir)
}

fn generate_random_files(output_dir: &Path, rng: &mut impl rand::Rng) -> Result<()> {
    let random_data: Vec<u8> = (0..1024).map(|_| rng.r#gen()).collect();

    let mut uid_file = File::create(output_dir.join("uid"))?;
    uid_file.write_all(&random_data)?;

    let mut uid_pub_file = File::create(output_dir.join("uid.pub"))?;
    uid_pub_file.write_all(&random_data)?;

    Ok(())
}

pub(crate) async fn populate_persistent(
    extracted_dir: PathBuf,
    persistent_img_path: PathBuf,
    mut rng: impl rand::Rng + Send + 'static,
) -> Result<()> {
    let persistent_img_path_clone = persistent_img_path.clone();
    tokio::task::spawn_blocking(move || {
        populate_persistent_inner(&extracted_dir, &persistent_img_path_clone, &mut rng)
    })
    .await
    .wrap_err("task panicked")?
    .wrap_err_with(|| {
        format!(
            "failed to populate the rts with the persistent images from \
            {persistent_img_path:?}"
        )
    })
}

// TODO: None of this needs to be sync, but I (@thebutlah) left it intact for now due
// to crunch.
fn populate_persistent_inner(
    extracted_dir: &Path,
    persistent_img_path: &Path,
    rng: &mut impl rand::Rng,
) -> Result<()> {
    let Some(bootloader_dir) = ["ready-to-sign", "rts"]
        .into_iter()
        .filter_map(|d| {
            let bootloader_dir = extracted_dir.join(d).join("bootloader");

            bootloader_dir
                .try_exists()
                .unwrap_or(false)
                .then_some(bootloader_dir)
        })
        .next()
    else {
        bail!("could not find a bootloader directory under {extracted_dir:?}");
    };

    // Copy .img files from persistent path to bootloader directory
    ensure!(
        persistent_img_path.try_exists().unwrap_or(false),
        "{persistent_img_path:?} doesn't exist"
    );

    let copy_result = run_cmd! {
        cp $persistent_img_path/persistent.img $bootloader_dir/ ;
        // writing persistent-journaled is necessary: tested with random persistent-journaled ->
        // leads to empty persistent
        cp $persistent_img_path/persistent-journaled.img $bootloader_dir/ ;
        sync;
    };

    copy_result
        .wrap_err("failed to copy .img files to bootloader directory")
        .with_note(|| format!("persistent_img_path was {persistent_img_path:?}, bootloader_dir was {bootloader_dir:?}"))?;

    // Generate random UID files
    generate_random_files(&bootloader_dir, rng)?;

    Ok(())
}

pub(crate) fn flash_cmd(variant: FlashVariant, extracted_dir: &Path) -> Result<()> {
    let Some(bootloader_dir) = ["ready-to-sign", "rts"]
        .into_iter()
        .filter_map(|d| {
            let bootloader_dir = extracted_dir.join(d).join("bootloader");

            bootloader_dir
                .try_exists()
                .unwrap_or(false)
                .then_some(bootloader_dir)
        })
        .next()
    else {
        bail!("could not find a bootloader directory under {extracted_dir:?}");
    };

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
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use std::fs;

    #[test]
    fn test_generate_random_files() -> Result<()> {
        // Create a temporary directory that will be cleaned up automatically
        let temp_dir = tempfile::TempDir::new()?;
        let output_dir = temp_dir.path();

        // Use a seeded RNG for deterministic testing
        let mut rng = StdRng::seed_from_u64(42);

        generate_random_files(output_dir, &mut rng)?;

        let uid_path = output_dir.join("uid");
        let uid_pub_path = output_dir.join("uid.pub");

        assert!(uid_path.exists(), "uid file should exist");
        assert!(uid_pub_path.exists(), "uid.pub file should exist");

        let uid_content = fs::read(&uid_path)?;
        let uid_pub_content = fs::read(&uid_pub_path)?;

        assert_eq!(uid_content.len(), 1024, "uid file should be 1024 bytes");
        assert_eq!(
            uid_pub_content.len(),
            1024,
            "uid.pub file should be 1024 bytes"
        );

        assert_eq!(
            uid_content, uid_pub_content,
            "uid and uid.pub should have identical content"
        );

        Ok(())
    }
}
