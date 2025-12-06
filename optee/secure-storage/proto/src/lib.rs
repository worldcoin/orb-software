#![no_std]

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u32)]
pub enum Command {
    Ping = 1,
    Echo = 2,
}

// If Uuid::parse_str() returns an InvalidLength error, there may be an extra
// newline in your uuid.txt file. You can remove it by running
// `truncate -s 36 uuid.txt`.
pub const UUID: &str = &include_str!("../../uuid.txt");
