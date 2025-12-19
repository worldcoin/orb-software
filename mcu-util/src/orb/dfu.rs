use std::cmp::min;
use std::fs::File;
use std::io;
use std::io::{Read, Seek};

use color_eyre::eyre::{eyre, Result};
use orb_mcu_interface::orb_messages;

/// Offset in bytes of the `struct image_version` field, in `struct image_header` in the binary file
/// see https://docs.mcuboot.com/design.html#image-format
const IMAGE_VERSION_OFFSET: u64 = 20;

/// Firmware version parsed from a binary file.
/// Corresponds to the MCU's image_version struct:
/// ```c
/// STRUCT_PACKED image_version {
///     uint8_t iv_major;
///     uint8_t iv_minor;
///     uint16_t iv_revision;
///     uint32_t iv_build_num;
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryFirmwareVersion {
    pub major: u8,
    pub minor: u8,
    pub revision: u16,
    pub build_num: u32,
}

impl std::fmt::Display for BinaryFirmwareVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "v{}.{}.{}-0x{:x}",
            self.major, self.minor, self.revision, self.build_num
        )
    }
}

impl BinaryFirmwareVersion {
    /// Check if this version matches a FirmwareVersion from the MCU.
    /// We use mcuboot's revision as patch number & build_num to store
    /// the commit hash
    pub fn matches(&self, fw: &orb_messages::FirmwareVersion) -> bool {
        self.major as u32 == fw.major
            && self.minor as u32 == fw.minor
            && self.revision as u32 == fw.patch
            && self.build_num == fw.commit_hash
    }
}

/// Parse the firmware version from a binary file at offset 20.
pub fn parse_firmware_version(buffer: &[u8]) -> Result<BinaryFirmwareVersion> {
    const VERSION_SIZE: usize = 8; // 1 + 1 + 2 + 4 bytes
    let offset = IMAGE_VERSION_OFFSET as usize;

    if buffer.len() < offset + VERSION_SIZE {
        return Err(eyre!(
            "Binary file too small to contain version info (need at least {} bytes, got {})",
            offset + VERSION_SIZE,
            buffer.len()
        ));
    }

    let major = buffer[offset];
    let minor = buffer[offset + 1];
    let revision = u16::from_le_bytes([buffer[offset + 2], buffer[offset + 3]]);
    let build_num = u32::from_le_bytes([
        buffer[offset + 4],
        buffer[offset + 5],
        buffer[offset + 6],
        buffer[offset + 7],
    ]);

    Ok(BinaryFirmwareVersion {
        major,
        minor,
        revision,
        build_num,
    })
}

/// One image can take up to 448KiB (Diamond), 224KiB (Pearl)
const MCU_MAX_FW_LEN: u64 = 448 * 1024;
const MCU_BLOCK_LEN: u64 = 39;

pub fn load_binary_file(path: &str) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.rewind()
        .map_err(|e| eyre!("failed seeking start of update binary file: {e}"))?;
    let src_len = file
        .seek(io::SeekFrom::End(0))
        .map_err(|e| eyre!("failed seeking end of update binary file: {e}"))?;
    file.rewind()
        .map_err(|e| eyre!("failed seeking start of update binary file: {e}"))?;

    assert!(src_len <= MCU_MAX_FW_LEN, "firmware size is too large");

    let mut buffer = Vec::with_capacity(src_len as usize); // Safe cast
    file.read_to_end(&mut buffer)
        .map_err(|e| eyre!("unable to load binary into vec: {e}"))?;

    Ok(buffer)
}

pub fn print_progress(percentage: f32) {
    print!("\r[");
    for i in 0..20 {
        if i as f32 / 20.0 * 100.0 < percentage {
            print!("=");
        } else {
            print!(" ");
        }
    }
    print!("] {}%\r", percentage as u32);
}

#[derive(Debug, Clone)]
pub struct BlockIterator<'a, I> {
    buffer: &'a [u8],
    block_num: u64,
    block_count: u64,
    _phantom: std::marker::PhantomData<I>,
}

impl<'a, I> BlockIterator<'a, I> {
    pub fn new(buffer: &'a [u8]) -> Self {
        let block_count = (buffer.len() as u64 - 1) / MCU_BLOCK_LEN + 1;
        BlockIterator::<'a, I> {
            buffer,
            block_num: 0,
            block_count,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn progress_percentage(&self) -> f32 {
        self.block_num as f32 / self.block_count as f32 * 100.0
    }
}
impl Iterator for BlockIterator<'_, orb_messages::main::jetson_to_mcu::Payload> {
    type Item = orb_messages::main::jetson_to_mcu::Payload;

    fn next(&mut self) -> Option<Self::Item> {
        if self.block_num < self.block_count {
            let start = (self.block_num * MCU_BLOCK_LEN) as usize;
            let end = ((self.block_num + 1) * MCU_BLOCK_LEN) as usize;
            let block = self.buffer[start..min(end, self.buffer.len())].to_vec();
            let dfu_block = Some(orb_messages::main::jetson_to_mcu::Payload::DfuBlock(
                orb_messages::FirmwareUpdateData {
                    block_number: self.block_num as u32,
                    block_count: self.block_count as u32,
                    image_block: block.to_vec(),
                },
            ));
            self.block_num += 1;
            dfu_block
        } else {
            None
        }
    }
}

impl Iterator for BlockIterator<'_, orb_messages::sec::jetson_to_sec::Payload> {
    type Item = orb_messages::sec::jetson_to_sec::Payload;

    fn next(&mut self) -> Option<Self::Item> {
        if self.block_num < self.block_count {
            let start = (self.block_num * MCU_BLOCK_LEN) as usize;
            let end = ((self.block_num + 1) * MCU_BLOCK_LEN) as usize;
            let block = self.buffer[start..min(end, self.buffer.len())].to_vec();
            let dfu_block = Some(orb_messages::sec::jetson_to_sec::Payload::DfuBlock(
                orb_messages::FirmwareUpdateData {
                    block_number: self.block_num as u32,
                    block_count: self.block_count as u32,
                    image_block: block.to_vec(),
                },
            ));
            self.block_num += 1;
            dfu_block
        } else {
            None
        }
    }
}

impl TryInto<orb_messages::main::jetson_to_mcu::Payload>
    for BlockIterator<'_, orb_messages::main::jetson_to_mcu::Payload>
{
    type Error = ();

    fn try_into(
        self,
    ) -> std::result::Result<orb_messages::main::jetson_to_mcu::Payload, Self::Error>
    {
        Ok(orb_messages::main::jetson_to_mcu::Payload::DfuBlock(
            orb_messages::FirmwareUpdateData {
                block_number: self.block_num as u32,
                block_count: self.block_count as u32,
                image_block: self.buffer.to_vec(),
            },
        ))
    }
}

impl TryInto<orb_messages::sec::jetson_to_sec::Payload>
    for BlockIterator<'_, orb_messages::sec::jetson_to_sec::Payload>
{
    type Error = ();

    fn try_into(
        self,
    ) -> std::result::Result<orb_messages::sec::jetson_to_sec::Payload, Self::Error>
    {
        Ok(orb_messages::sec::jetson_to_sec::Payload::DfuBlock(
            orb_messages::FirmwareUpdateData {
                block_number: self.block_num as u32,
                block_count: self.block_count as u32,
                image_block: self.buffer.to_vec(),
            },
        ))
    }
}
