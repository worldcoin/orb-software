use std::ffi::{c_char, c_int};

pub type Hash = [u8; 32];

#[repr(C)]
pub struct Block {
    pub offset: u64,
    pub size: u32,
    pub hash: Hash, //sha256
}

unsafe extern "C" {
    pub fn shim_get_blocks(
        path: *const c_char,
        blocks: *mut *mut Block,
        blocks_len: *mut usize,
    ) -> c_int;

    pub fn shim_get_inode_table_idx(path: *const c_char) -> u64;
}
