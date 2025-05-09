//! Efivar module to read and write efivar files.
//! Submodules represent data for worldcoin fork of the Nvidia's EDK II implementation.
//!
//! [Nvidia Source](https://github.com/NVIDIA/edk2-nvidia)
//!
//! [Worldcoin Fork](https://github.com/worldcoin/edk2-nvidia)
//!
//! [efivar Documentation](https://www.kernel.org/doc/html/latest/filesystems/efivarfs.html)

use std::{
    ffi::c_int,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};
use thiserror::Error;

mod ioctl;
use color_eyre::{eyre::Context, Result};

const EFIVARS_PATH: &str = "sys/firmware/efi/efivars/";

#[derive(Error, Debug)]
pub enum EfiVarDbErr {
    #[error("Failed to canonicalize EfiVarDb path")]
    FailedCanonicalization(#[from] io::Error),
    #[error("EfiVar path cannot be absolute. Given '{0:?}'")]
    VarPathCannotBeAbsolute(PathBuf),
}

pub struct EfiVarDb {
    pub path: PathBuf,
}

impl EfiVarDb {
    /// Retuns an [`EfiVarDb`] for the given rootfs.
    /// Does blocking checks on the filesystem.
    pub fn from_rootfs(rootfs_path: impl AsRef<Path>) -> Result<Self, EfiVarDbErr> {
        let path = rootfs_path.as_ref().join(EFIVARS_PATH);
        let path = fs::canonicalize(path)?;

        Ok(Self { path })
    }

    pub fn get_var(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<EfiVar, EfiVarDbErr> {
        let relative_path = relative_path.as_ref();
        if relative_path.is_absolute() {
            return Err(EfiVarDbErr::VarPathCannotBeAbsolute(relative_path.into()));
        }

        let path = self.path.join(relative_path);

        Ok(EfiVar { path })
    }

    /// Returns the filesystem path to this [`EfiVarDb`].
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

/// Efivar representation.
pub struct EfiVar {
    // Path to efivar.
    path: PathBuf,
}

impl EfiVar {
    /// Read the efivar data from a `path`.
    pub fn read(&self) -> Result<Vec<u8>> {
        fs::read(&self.path).wrap_err_with(|| format!("Failed to read {:?}", self.path))
    }

    /// This function will create a efi file if it does not exist,
    /// Will entirely replace its contents if it does.
    pub fn write(&self, buffer: &[u8]) -> Result<()> {
        let err = |msg| move || format!("Failed to {msg} {:?}", self.path);

        match File::open(&self.path) {
            Ok(file_read) => {
                let original_attributes: c_int =
                    ioctl::read_file_attributes(&file_read)
                        .wrap_err_with(err("read file attributes"))?;

                // Make file mutable.
                let new_attributes = original_attributes & !ioctl::IMMUTABLE_MASK;
                ioctl::write_file_attributes(&file_read, new_attributes)
                    .wrap_err_with(err("make file mutalbe"))?;

                fs::write(&self.path, buffer).wrap_err_with(err("write to file"))?;

                // Make file immutable again.
                ioctl::write_file_attributes(&file_read, original_attributes)
                    .wrap_err_with(err("make file immutable"))
            }

            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::write(&self.path, buffer).wrap_err_with(err("write to file"))
            }

            Err(e) => Err(e).wrap_err_with(err("open file")),
        }
    }

    /// Remove UEFI variable
    pub fn remove(&self) -> Result<()> {
        std::fs::remove_file(&self.path)
            .wrap_err_with(|| format!("Failed to remove file {:?}", self.path))
    }
}
