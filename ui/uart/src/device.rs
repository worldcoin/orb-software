use super::{close, ioctl, open, read, tcdrain, tcflush, tcgetattr, tcsetattr, write};
use libc::{
    c_int, speed_t, termios, CLOCAL, CREAD, CRTSCTS, CS8, CSIZE, ECHO, ECHOCTL, ECHOE,
    ECHOK, ECHOKE, ECHONL, ICANON, ICRNL, IEXTEN, IGNBRK, IGNCR, INLCR, INPCK, ISIG,
    ISTRIP, IXANY, IXOFF, IXON, OCRNL, ONLCR, OPOST, O_CLOEXEC, O_EXCL, O_NOCTTY,
    O_RDWR, PARENB, PARMRK, PARODD, TCIFLUSH, TCSANOW, TIOCMBIS, TIOCM_DTR, TIOCM_RTS,
    VMIN, VTIME,
};
use std::{ffi::CString, io, mem, os::unix::ffi::OsStrExt, path::Path, ptr};

/// Bi-directional UART handle.
#[derive(Clone)]
pub struct Device {
    fd: c_int,
}

/// Serial interface baud rate.
#[derive(Clone, Copy)]
#[repr(u64)]
pub enum BaudRate {
    /// Baud rate 0.
    B0 = libc::B0 as _,
    /// Baud rate 9600.
    B9600 = libc::B9600 as _,
    /// Baud rate 19200.
    B19200 = libc::B19200 as _,
    /// Baud rate 38400.
    B38400 = libc::B38400 as _,
    /// Baud rate 57600.
    B57600 = libc::B57600 as _,
    /// Baud rate 115200.
    B115200 = libc::B115200 as _,
    /// Baud rate 230400.
    B230400 = libc::B230400 as _,
    /// Baud rate 460800.
    #[cfg(target_os = "linux")]
    B460800 = libc::B460800 as _,
    /// Baud rate 500000.
    #[cfg(target_os = "linux")]
    B500000 = libc::B500000 as _,
    /// Baud rate 576000.
    #[cfg(target_os = "linux")]
    B576000 = libc::B576000 as _,
    /// Baud rate 921600.
    #[cfg(target_os = "linux")]
    B921600 = libc::B921600 as _,
    /// Baud rate 1000000.
    #[cfg(target_os = "linux")]
    B1000000 = libc::B1000000 as _,
}

impl BaudRate {
    fn to_speed(self) -> speed_t {
        self as _
    }
}

impl Device {
    /// Opens a serial interface.
    ///
    /// # Panics
    ///
    /// If failed to open the device.
    pub fn open<T: AsRef<Path>>(path: T, baud_rate: BaudRate) -> io::Result<Self> {
        let baud_rate = baud_rate.to_speed();
        let path = CString::new(path.as_ref().as_os_str().as_bytes())?;
        let fd = unsafe {
            open(path.as_ptr(), O_RDWR | O_NOCTTY | O_EXCL | O_CLOEXEC).unwrap()
        };

        let mut termios: termios = unsafe { mem::zeroed() };
        unsafe { tcgetattr(fd, &mut termios)? };
        termios.c_cflag &= !(PARENB | PARODD | CSIZE | CRTSCTS);
        #[cfg(target_os = "linux")]
        {
            termios.c_cflag &= !(libc::CMSPAR | libc::CBAUD);
        }
        termios.c_cflag |= CLOCAL | CREAD | CS8 | baud_rate;
        termios.c_lflag &= !(ICANON
            | ECHO
            | ECHOE
            | ECHOK
            | ECHONL
            | ISIG
            | IEXTEN
            | ECHOCTL
            | ECHOKE);
        termios.c_oflag &= !(OPOST | ONLCR | OCRNL);
        termios.c_iflag &= !(INLCR
            | IGNCR
            | ICRNL
            | IGNBRK
            | INPCK
            | ISTRIP
            | IXON
            | IXOFF
            | IXANY
            | PARMRK);
        termios.c_ispeed = baud_rate;
        termios.c_ospeed = baud_rate;
        termios.c_cc[VMIN] = 1;
        termios.c_cc[VTIME] = 0;
        unsafe { tcsetattr(fd, TCSANOW, &termios)? };

        let mut bits: c_int = TIOCM_DTR | TIOCM_RTS;
        unsafe { ioctl(fd, TIOCMBIS, ptr::addr_of_mut!(bits).cast())? };
        unsafe { tcflush(fd, TCIFLUSH)? };

        Ok(Self { fd })
    }
}

impl io::Read for Device {
    #[allow(clippy::cast_sign_loss)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = unsafe { read(self.fd, buf.as_mut_ptr().cast(), buf.len())? };
        Ok(read as _)
    }
}

impl io::Write for Device {
    #[allow(clippy::cast_sign_loss)]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = unsafe { write(self.fd, buf.as_ptr().cast(), buf.len())? };
        Ok(written as _)
    }

    fn flush(&mut self) -> io::Result<()> {
        unsafe { tcdrain(self.fd) }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            if let Err(err) = close(self.fd) {
                log::error!("Couldn't close serial interface descriptor: {err}");
            }
        }
    }
}
