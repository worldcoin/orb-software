use std::fs::File;
use std::io::Write;
use std::path::Path;

use camino::Utf8Path;
use cmd_lib::run_cmd;
use color_eyre::{
    eyre::{ensure, WrapErr},
    Result, Section,
};
use tempfile::TempDir;

use crate::boot::is_recovery_mode_detected;

pub async fn flash(
    variant: FlashVariant,
    path_to_rts_tar: &Utf8Path,
    persistent_img_path: &Path,
) -> Result<()> {
    let path_to_rts = path_to_rts_tar.to_owned();
    let persistent_img_path = persistent_img_path.to_owned();
    tokio::task::spawn_blocking(move || {
        ensure!(is_recovery_mode_detected()?, "orb not in recovery mode");
        let tmp_dir = extract(&path_to_rts)?;
        println!("{tmp_dir:?}");
        ensure!(is_recovery_mode_detected()?, "orb not in recovery mode");
        flash_cmd(variant, tmp_dir.path(), &persistent_img_path)?;
        Ok(())
    })
    .await
    .wrap_err("task panicked")?
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FlashVariant {
    Fast,
    Regular,
    HilFast,
    Hil,
}

impl FlashVariant {
    fn file_name(&self) -> &'static str {
        match self {
            FlashVariant::Fast => "fastflashcmd.txt",
            FlashVariant::Regular => "flashcmd.txt",
            FlashVariant::HilFast => "hil-fastflashcmd.txt",
            FlashVariant::Hil => "hil-flashcmd.txt",
        }
    }
}

fn extract(path_to_rts: &Utf8Path) -> Result<TempDir> {
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

fn flash_cmd(
    variant: FlashVariant,
    extracted_dir: &Path,
    persistent_img_path: &Path,
) -> Result<()> {
    let bootloader_dir = extracted_dir.join("ready-to-sign").join("bootloader");
    ensure!(
        bootloader_dir.try_exists().unwrap_or(false),
        "{bootloader_dir:?} doesn't exist"
    );

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

    let cmd_file_name = variant.file_name();

    // Generate random UID files
    generate_random_files(&bootloader_dir, &mut rand::thread_rng())?;

    let result = run_cmd! {
        cd $bootloader_dir;
        info "Removing fetch persistent commands from flash script";
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
