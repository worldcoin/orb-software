use crate::sys;

pub type Result<T> = core::result::Result<T, ErrorCode>;

/// Allows us to just paste the PascalCase version of the error codes in a macro.
macro_rules! err_variants {
    ($($variant:ident,)+) => {
        #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
        #[repr(i32)]
        pub enum ErrorCode {
            $($variant = sys::error_t::$variant.0),+
        }
        impl ErrorCode {
            /// Returns `None` on a [`sys::error_t::Success`].
            /// Will panic if `err` is an unknown value.
            pub fn new_from_sys(err: sys::error_t) -> Option<Self> {
                match err {
                    $(sys::error_t::$variant => Some(Self::$variant),)+
                    sys::error_t(0) => None,
                    err => panic!("Unexpected sys::error_t value! Got {err:?}"),
                }
            }

            pub fn result_from_sys(err: sys::error_t) -> Result<()> {
                match Self::new_from_sys(err) {
                    Some(ec) => Err(ec),
                    None => Ok(()),
                }
            }
        }
    };
}

impl core::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ErrorCode {}

err_variants! {
    DeviceCommunication,
    InvalidParameter,
    Permissions,
    NoDevice,
    DeviceNotFound,
    DeviceBusy,
    Timeout,
    Overflow,
    UnknownRequest,
    Interrupted,
    OutOfMemory,
    NotSupported,
    Other,
    CannotPerformRequest,
    FlashAccessFailure,
    ImplementationError,
    RequestPending,
    InvalidFirmwareImage,
    InvalidKey,
    SensorCommunication,
    OutOfRange,
    VerifyFailed,
    SyscallFailed,
    FileDoesNotExist,
    DirectoryDoesNotExist,
    FileReadFailed,
    FileWriteFailed,
    NotImplemented,
    NotPaired,
}
