//! Efivar module to read and write efivar files.
//! [efivar Documentation](https://www.kernel.org/doc/html/latest/filesystems/efivarfs.html)

use std::{
    ffi::c_int,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

mod ioctl;
use color_eyre::{
    eyre::{bail, eyre, Context},
    Result,
};

const EFIVARS_PATH: &str = "sys/firmware/efi/efivars/";

pub struct EfiVarDb {
    pub path: PathBuf,
}

impl EfiVarDb {
    /// Retuns an [`EfiVarDb`] for the given rootfs.
    /// Does blocking checks on the filesystem.
    pub fn from_rootfs(rootfs_path: impl AsRef<Path>) -> Result<Self> {
        let path = rootfs_path.as_ref().join(EFIVARS_PATH);
        let path = fs::canonicalize(path)
            .map_err(|e| eyre!("Failed to canonicalize EfiVarDb path: {e}"))?;

        Ok(Self { path })
    }

    pub fn get_var(&self, relative_path: impl AsRef<Path>) -> Result<EfiVar> {
        let relative_path = relative_path.as_ref();
        if relative_path.is_absolute() {
            bail!("EfiVar path cannot be absolute. Given '{relative_path:?}'");
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
    pub fn read(&self) -> Result<EfiVarData> {
        fs::read(&self.path)
            .wrap_err_with(|| format!("Failed to read {:?}", self.path))
            .and_then(EfiVarData::from_bytes)
    }

    /// This function will create a efi file if it does not exist,
    /// Will entirely replace its contents if it does.
    pub fn write(&self, efi_var_data: &EfiVarData) -> Result<()> {
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

                fs::write(&self.path, efi_var_data.as_bytes())
                    .wrap_err_with(err("write to file"))?;

                // Make file immutable again.
                ioctl::write_file_attributes(&file_read, original_attributes)
                    .wrap_err_with(err("make file immutable"))
            }

            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::write(&self.path, efi_var_data.as_bytes())
                    .wrap_err_with(err("write to file"))
            }

            Err(e) => Err(e).wrap_err_with(err("open file")),
        }
    }

    /// Remove UEFI variable
    pub fn remove(&self) -> Result<()> {
        let err = |msg| move || format!("Failed to {msg} {:?}", self.path);

        if let Ok(file) = File::open(&self.path) {
            let original_attributes: c_int = ioctl::read_file_attributes(&file)
                .wrap_err_with(err("read file attributes"))?;

            // Make file mutable.
            let new_attributes = original_attributes & !ioctl::IMMUTABLE_MASK;
            ioctl::write_file_attributes(&file, new_attributes)
                .wrap_err_with(err("make file mutalbe"))?;

            fs::remove_file(&self.path).wrap_err_with(err("remove file"))?;
        }

        Ok(())
    }
}

/// Represents EFI variable data as exposed through the Linux efivars filesystem.
///
/// EFI variables in Linux are exposed as binary blobs in /sys/firmware/efi/efivars/,
/// where the first 4 bytes represent attributes and the remaining bytes represent
/// the actual variable data. This struct provides methods to work with such data.
///
/// Format: `[4 bytes attributes][n bytes value]`
///
/// The attributes are a bitmask in the first byte where:
/// - Bit 0 (0x01): EFI_VARIABLE_NON_VOLATILE
/// - Bit 1 (0x02): EFI_VARIABLE_BOOTSERVICE_ACCESS
/// - Bit 2 (0x04): EFI_VARIABLE_RUNTIME_ACCESS
/// - Other bits: Used for other attributes (rarely used in NVIDIA EDK2)
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct EfiVarData(Vec<u8>);

impl EfiVarData {
    /// The attributes are a bitmask in the first byte where:
    /// - Bit 0 (0x01): EFI_VARIABLE_NON_VOLATILE
    /// - Bit 1 (0x02): EFI_VARIABLE_BOOTSERVICE_ACCESS
    /// - Bit 2 (0x04): EFI_VARIABLE_RUNTIME_ACCESS
    /// # Example
    ///
    /// ```
    /// use efivar::EfiVarData;
    ///
    /// // 0x07 sets all three flags: NON_VOLATILE | BOOTSERVICE_ACCESS | RUNTIME_ACCESS
    /// let data = EfiVarData::new(0x07, &[0x1, 0x0, 0x0, 0x0]);
    ///
    /// assert_eq!(
    ///     data.as_bytes(),
    ///     &[0x07, 0x00, 0x00, 0x00, 0x1, 0x0, 0x0, 0x0]
    /// );
    /// ```
    pub fn new(attributes: u8, value: impl AsRef<[u8]>) -> EfiVarData {
        let value = value.as_ref();
        let attributes = [attributes, 0x0, 0x0, 0x0];

        let mut vec = Vec::with_capacity(value.len() + attributes.len());
        vec.extend_from_slice(&attributes);
        vec.extend_from_slice(value);

        EfiVarData(vec)
    }

    /// Creates a new EfiVarData from raw bytes.
    ///
    /// The input must be at least 4 bytes long:
    /// - First 4 bytes: Attributes
    /// - All bytes after first 4: Value
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Result<EfiVarData> {
        let bytes = bytes.as_ref();
        let len = bytes.len();

        if len < 4 {
            bail!(
                "EFI variable data must be at least 4 bytes in size, got {}",
                len
            );
        }

        Ok(EfiVarData(bytes.to_vec()))
    }

    /// Checks if the EFI_VARIABLE_NON_VOLATILE attribute is set.
    /// If true then the variable is stored in non-volatile memory
    pub fn non_volatile(&self) -> bool {
        (self.0[0] & 0x01) != 0
    }

    /// Checks if the EFI_VARIABLE_BOOTSERVICE_ACCESS attribute is set.
    /// If true then the variable is accessible during boot services
    pub fn bootservices_access(&self) -> bool {
        (self.0[0] & 0x02) != 0
    }

    /// Checks if the EFI_VARIABLE_RUNTIME_ACCESS attribute is set.
    /// If true then the variable is accessible at runtime
    pub fn runtime_access(&self) -> bool {
        (self.0[0] & 0x04) != 0
    }

    /// Returns the variable value (without attributes)
    pub fn value(&self) -> &[u8] {
        &self.0[4..]
    }

    /// Returns the entire EFI variable data as an 8-byte array.
    /// Contains attributes + EFI variable value.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
