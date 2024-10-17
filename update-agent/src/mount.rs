use std::{
    ffi::CString,
    ops::Drop,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

const DEV_DISK_BY_PARTLABEL: &str = "/dev/disk/by-partlabel/";
use tracing::debug;

/// Temporary mount point for a device. When the `TemporaryMount` is dropped,
/// the mount point is unmounted.
pub struct TemporaryMount {
    mount_point: tempfile::TempDir,
}

impl TemporaryMount {
    pub fn new<P: AsRef<Path>>(device: P) -> std::io::Result<Self> {
        let mount_point = tempfile::tempdir()?;
        let device = device.as_ref().canonicalize()?;
        sys_mount(&device, mount_point.path())?;
        Ok(Self { mount_point })
    }

    // Create a file in the temporary mount point.
    // The file *is not* automatically deleted when the `TemporaryMount` is dropped.
    // `path` should be a relative path but it could include directories that don't exist.
    //
    // # Panics
    // If `path` is not a relative path.
    pub fn create_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> std::io::Result<std::fs::File> {
        assert!(path.as_ref().is_relative());
        let absolute_path = self.mount_point.path().join(path.as_ref());
        std::fs::create_dir_all(absolute_path.parent().unwrap())?;
        std::fs::File::create(absolute_path)
    }
}

impl Drop for TemporaryMount {
    fn drop(&mut self) {
        let _ = sys_umount(self.mount_point.path(), false);
    }
}

// Opinionated Rust wrapper over mount(2)
fn sys_mount(src: &Path, dst: &Path) -> std::io::Result<()> {
    let src = CString::new(src.as_os_str().as_bytes())?;
    let dst = CString::new(dst.as_os_str().as_bytes())?;
    let err = unsafe {
        libc::mount(
            src.as_ptr(),
            dst.as_ptr(),
            CString::new("vfat")?.as_ptr(),
            libc::MS_NOEXEC | libc::MS_NOSUID | libc::MS_NODEV,
            std::ptr::null(),
        )
    };

    match err {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

fn sys_umount(path: &Path, lazy: bool) -> std::io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    let mut flags = libc::UMOUNT_NOFOLLOW;
    if lazy {
        flags |= libc::MNT_DETACH;
    }
    let err = unsafe { libc::umount2(path.as_ptr(), flags) };

    match err {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

// Append a filename to a path. If the filename is not a file name but a
// directory or a relative path, fail.
fn append_filename_to_path<P: AsRef<Path>>(
    basepath: P,
    filename: &str,
) -> std::io::Result<PathBuf> {
    let mut path = basepath.as_ref().to_path_buf().join(filename);
    path = path.canonicalize()?;
    if !path.starts_with(basepath) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "tried to hack us by un-mounting layers, eh?",
        ));
    }
    Ok(path)
}

pub fn unmount_partition_by_label(label: &str) -> std::io::Result<()> {
    let device_path = append_filename_to_path(DEV_DISK_BY_PARTLABEL, label)?;
    debug!("calling `umount -l {}`", device_path.display());
    sys_umount(&device_path, true)
}
