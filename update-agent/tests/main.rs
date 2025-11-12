#![allow(clippy::uninlined_format_args)]
use std::{
    fs::File,
    io::{Read, Seek},
};

use orb_update_agent_core::components::Gpt;

#[ignore = "requires specific block device"]
#[test]
fn test_blockdevice_size() {
    let _ = orb_telemetry::TelemetryConfig::new().init();

    let mut block_device: File = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(false)
        .open(std::path::PathBuf::from("/dev/disk2"))
        .expect("failed to open block device");

    let mut mbr = vec![0u8; 512];
    block_device
        .read_exact(&mut mbr)
        .expect("failed to read MBR block");
    println!("{:X?}", mbr);

    block_device.seek(std::io::SeekFrom::Start(512)).unwrap();

    let mut gpt = vec![0u8; 512];
    block_device
        .read_exact(&mut gpt)
        .expect("failed to read GPT block");

    block_device.seek(std::io::SeekFrom::Start(0)).unwrap();

    let _disk = gpt::GptConfig::new()
        .writable(false)
        .logical_block_size(Gpt::LOGICAL_BLOCK_SIZE)
        .open_from_device(Box::new(block_device))
        .expect("failed to open target GPT device");
}
