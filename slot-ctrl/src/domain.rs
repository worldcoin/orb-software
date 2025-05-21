use derive_more::Display;
use efivar::EfiVarDbErr;
use orb_info::orb_os_release::OrbType;

pub type Result<T> = std::result::Result<T, Error>;

/// Error definition for library.
#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed reading efivar, invalid data length. expected: {expected}, actual: {actual}")]
    InvalidEfiVarLen { expected: usize, actual: usize },
    #[error("invalid slot configuration")]
    InvalidSlotData,
    #[error("invalid rootfs status")]
    InvalidRootFsStatusData,
    #[error("invalid retry counter({counter}), exceeding the maximum ({max})")]
    ExceedingRetryCount { counter: u8, max: u8 },
    #[error("{0}")]
    EfiVar(#[from] color_eyre::Report),
    #[error("{0}")]
    EfiVarDb(#[from] EfiVarDbErr),
    #[error("unsupported orb type: {0}")]
    UnsupportedOrbType(OrbType),
    #[error("{0}")]
    Verification(String),
}

/// Representation of the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub enum Slot {
    #[display("a")]
    A,
    #[display("b")]
    B,
}

/// Representation of the rootfs status.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RootFsStatus {
    /// Default status of the rootfs.
    Normal,
    /// Status of the rootfs where the partitions during an update are written.
    UpdateInProcess,
    /// Status of the rootfs where the boot slot was just switched to it.
    UpdateDone,
    /// Status of the rootfs is considered unbootable.
    Unbootable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
pub struct RetryCount(pub u8);

impl RetryCount {
    /// RetryCount as EfiVar raw bytes
    pub fn as_bytes(&self) -> [u8; 8] {
        [0x07, 0x00, 0x00, 0x00, self.0, 0x00, 0x00, 0x00]
    }

    /// RetryCount from EfiVar raw bytes
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Result<RetryCount> {
        let bytes = bytes.as_ref();
        let len = bytes.len();
        if len != 8 {
            return Err(Error::InvalidEfiVarLen {
                expected: 8,
                actual: len,
            });
        }

        // In Linux EFI variables have 8 bytes. The first 4 bytes attributes.
        // The other 4 bytes are the value of the variable itself.
        // So 5th byte is the first carrying EFI var value (little endian repr)
        Ok(RetryCount(bytes[4]))
    }
}

impl Slot {
    // In Linux EFI variables have 8 bytes. The first 4 bytes attributes.
    // The other 4 bytes are the value of the variable itself.
    // In this case, 0x07 corresponds to:
    // - EFI_VARIABLE_NON_VOLATILE (0x01)
    // - EFI_VARIABLE_BOOTSERVICE_ACCESS (0x02)
    // - EFI_VARIABLE_RUNTIME_ACCESS (0x04)
    const SLOT_A_BYTES: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const SLOT_B_BYTES: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];

    pub const CURRENT_SLOT_PATH: &str =
        "BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9";
    pub const NEXT_SLOT_PATH: &str =
        "BootChainFwNext-781e084c-a330-417c-b678-38e696380cb9";

    /// Slot as EfiVar raw bytes
    pub fn as_bytes(&self) -> &'static [u8; 8] {
        match self {
            Slot::A => &Self::SLOT_A_BYTES,
            Slot::B => &Self::SLOT_B_BYTES,
        }
    }

    /// Slot from EfiVar raw bytes
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Result<Slot> {
        let bytes = bytes.as_ref();
        if Slot::SLOT_A_BYTES == bytes {
            Ok(Slot::A)
        } else if Slot::SLOT_B_BYTES == bytes {
            Ok(Slot::B)
        } else {
            Err(Error::InvalidSlotData)
        }
    }
}

impl RootFsStatus {
    // In Linux EFI variables have 8 bytes. The first 4 bytes attributes.
    // The other 4 bytes are the value of the variable itself.
    // In this case, 0x07 corresponds to:
    // - EFI_VARIABLE_NON_VOLATILE (0x01)
    // - EFI_VARIABLE_BOOTSERVICE_ACCESS (0x02)
    // - EFI_VARIABLE_RUNTIME_ACCESS (0x04)

    // Right now Pearl has extra states in the update status, some thing
    // we will probably get rid of in the future. Values were also altered and are
    // different than the default NVIDIA ones (used by Diamond)
    // https://github.com/worldcoin/edk2-nvidia/blob/ede09eb66b00d5d185ba93b7992390f2a483b46f/Silicon/NVIDIA/Include/NVIDIAConfiguration.h#L23
    const PEARL_NORMAL: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const PEARL_UPDATE_IN_PROGRESS: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    const PEARL_UPDATE_DONE: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
    const PEARL_UNBOOTABLE: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00];

    // https://github.com/worldcoin/edk2-nvidia/blob/86a32d95373d6aaf87278093a855ccf193b9c61f/Silicon/NVIDIA/Include/NVIDIAConfiguration.h#L23
    const DIAMOND_NORMAL: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const DIAMOND_UNBOOTABLE: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00];

    pub const STATUS_A_PATH: &str =
        "RootfsStatusSlotA-781e084c-a330-417c-b678-38e696380cb9";
    pub const STATUS_B_PATH: &str =
        "RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9";
    pub const RETRY_COUNT_A_PATH: &str =
        "RootfsRetryCountA-781e084c-a330-417c-b678-38e696380cb9";
    pub const RETRY_COUNT_B_PATH: &str =
        "RootfsRetryCountB-781e084c-a330-417c-b678-38e696380cb9";
    pub const RETRY_COUNT_MAX_PATH: &str =
        "RootfsRetryCountMax-781e084c-a330-417c-b678-38e696380cb9";

    /// RootFsStatus as EfiVar raw bytes
    pub fn as_bytes(&self, orb: OrbType) -> Result<&'static [u8; 8]> {
        match (self, orb) {
            (Self::Normal, OrbType::Pearl) => Ok(&Self::PEARL_NORMAL),
            (Self::UpdateInProcess, OrbType::Pearl) => {
                Ok(&Self::PEARL_UPDATE_IN_PROGRESS)
            }
            (Self::UpdateDone, OrbType::Pearl) => Ok(&Self::PEARL_UPDATE_DONE),
            (Self::Unbootable, OrbType::Pearl) => Ok(&Self::PEARL_UNBOOTABLE),
            (Self::Normal, OrbType::Diamond) => Ok(&Self::DIAMOND_NORMAL),
            (Self::Unbootable, OrbType::Diamond) => Ok(&Self::DIAMOND_UNBOOTABLE),
            _ => Err(Error::InvalidRootFsStatusData),
        }
    }

    /// RootFsStatus from EfiVar raw bytes
    pub fn from_bytes(bytes: impl AsRef<[u8]>, orb: OrbType) -> Result<RootFsStatus> {
        let bytes = bytes.as_ref();

        match orb {
            OrbType::Pearl if bytes == Self::PEARL_NORMAL => Ok(RootFsStatus::Normal),

            OrbType::Pearl if bytes == Self::PEARL_UPDATE_IN_PROGRESS => {
                Ok(RootFsStatus::UpdateInProcess)
            }

            OrbType::Pearl if bytes == Self::PEARL_UPDATE_DONE => {
                Ok(RootFsStatus::UpdateDone)
            }

            OrbType::Pearl if bytes == Self::PEARL_UNBOOTABLE => {
                Ok(RootFsStatus::Unbootable)
            }

            OrbType::Diamond if bytes == Self::DIAMOND_NORMAL => {
                Ok(RootFsStatus::Normal)
            }

            OrbType::Diamond if bytes == Self::DIAMOND_NORMAL => {
                Ok(RootFsStatus::Unbootable)
            }

            _ => Err(Error::InvalidRootFsStatusData),
        }
    }
}
