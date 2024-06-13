use color_eyre::eyre::{eyre, Result};
use orb_mcu_interface::orb_messages::mcu_main as main_messaging;
use orb_mcu_interface::orb_messages::mcu_sec as sec_messaging;
use std::cmp::min;
use std::fs::File;
use std::io;
use std::io::{Read, Seek};

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
impl Iterator for BlockIterator<'_, main_messaging::jetson_to_mcu::Payload> {
    type Item = main_messaging::jetson_to_mcu::Payload;

    fn next(&mut self) -> Option<Self::Item> {
        if self.block_num < self.block_count {
            let start = (self.block_num * MCU_BLOCK_LEN) as usize;
            let end = ((self.block_num + 1) * MCU_BLOCK_LEN) as usize;
            let block = self.buffer[start..min(end, self.buffer.len())].to_vec();
            self.block_num += 1;
            Some(main_messaging::jetson_to_mcu::Payload::DfuBlock(
                main_messaging::FirmwareUpdateData {
                    block_number: self.block_num as u32,
                    block_count: self.block_count as u32,
                    image_block: block.to_vec(),
                },
            ))
        } else {
            None
        }
    }
}

impl Iterator for BlockIterator<'_, sec_messaging::jetson_to_sec::Payload> {
    type Item = sec_messaging::jetson_to_sec::Payload;

    fn next(&mut self) -> Option<Self::Item> {
        if self.block_num < self.block_count {
            let start = (self.block_num * MCU_BLOCK_LEN) as usize;
            let end = ((self.block_num + 1) * MCU_BLOCK_LEN) as usize;
            let block = self.buffer[start..min(end, self.buffer.len())].to_vec();
            self.block_num += 1;
            Some(sec_messaging::jetson_to_sec::Payload::DfuBlock(
                sec_messaging::FirmwareUpdateData {
                    block_number: self.block_num as u32,
                    block_count: self.block_count as u32,
                    image_block: block.to_vec(),
                },
            ))
        } else {
            None
        }
    }
}

impl TryInto<main_messaging::jetson_to_mcu::Payload>
    for BlockIterator<'_, main_messaging::jetson_to_mcu::Payload>
{
    type Error = ();

    fn try_into(
        self,
    ) -> std::result::Result<main_messaging::jetson_to_mcu::Payload, Self::Error> {
        Ok(main_messaging::jetson_to_mcu::Payload::DfuBlock(
            main_messaging::FirmwareUpdateData {
                block_number: self.block_num as u32,
                block_count: self.block_count as u32,
                image_block: self.buffer.to_vec(),
            },
        ))
    }
}

impl TryInto<sec_messaging::jetson_to_sec::Payload>
    for BlockIterator<'_, sec_messaging::jetson_to_sec::Payload>
{
    type Error = ();

    fn try_into(
        self,
    ) -> std::result::Result<sec_messaging::jetson_to_sec::Payload, Self::Error> {
        Ok(sec_messaging::jetson_to_sec::Payload::DfuBlock(
            sec_messaging::FirmwareUpdateData {
                block_number: self.block_num as u32,
                block_count: self.block_count as u32,
                image_block: self.buffer.to_vec(),
            },
        ))
    }
}
