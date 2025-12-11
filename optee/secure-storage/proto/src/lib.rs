#![no_std]

use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Debug, Clone, Copy, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum CommandId {
    Ping = 1,
    Echo = 2,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Command {
    Ping,
    Echo(u32),
}

// If Uuid::parse_str() returns an InvalidLength error, there may be an extra
// newline in your uuid.txt file. You can remove it by running
// `truncate -s 36 uuid.txt`.
pub const UUID: &str = include_str!("../../uuid.txt");
