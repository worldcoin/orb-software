use std::{
    collections::HashMap,
    ffi::{c_char, CString},
    io::{self, Write},
    path::Path,
};

use bidiff::{diff, DiffParams, Match, Translator};
use orb_bidiff_squashfs_shim::{
    shim_get_blocks, shim_get_inode_table_idx, Block, Hash,
};

fn get_inode_table_idx(path: &Path) -> Result<usize, std::io::Error> {
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let ret = unsafe { shim_get_inode_table_idx(c_path.as_ptr() as *const c_char) };
    if ret == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(ret as usize)
}

struct Fragments {
    data: Vec<Block>,
    pos: usize,
}

impl Fragments {
    fn new(path: &Path) -> Result<Self, std::io::Error> {
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut blocks = std::ptr::null_mut();
        let mut blocks_len = 0usize;
        let ret = unsafe {
            shim_get_blocks(
                c_path.as_ptr() as *const c_char,
                &mut blocks as *mut *mut Block,
                &mut blocks_len as *mut usize,
            )
        };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let data = unsafe { Vec::from_raw_parts(blocks, blocks_len, blocks_len) };
        Ok(Self { data, pos: 0 })
    }
}

impl Iterator for Fragments {
    type Item = (Hash, u64, u32); //Hash & offset & size
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.data.len() {
            let block = &self.data[self.pos];
            self.pos += 1;
            Some((block.hash, block.offset, block.size))
        } else {
            None
        }
    }
}

fn diff_squashfs_data<F>(
    old_path: &Path,
    new_path: &Path,
    mut on_match: F,
) -> Result<(), io::Error>
where
    F: FnMut(Match) -> Result<(), io::Error>,
{
    let old_map = Fragments::new(old_path)?
        .map(|(hash, pos, length)| (hash, (pos, length)))
        .collect::<HashMap<Hash, (u64, u32)>>();

    for (new_hash, new_pos, length) in Fragments::new(new_path).unwrap() {
        let m = match old_map.get(&new_hash) {
            Some((old_pos, old_length)) => {
                assert_eq!(length, *old_length);
                Match {
                    add_old_start: *old_pos as usize,
                    add_new_start: new_pos as usize,
                    add_length: length as usize,
                    copy_end: (new_pos + length as u64) as usize,
                }
            }
            None => Match {
                add_old_start: 0,
                add_new_start: new_pos as usize,
                add_length: 0,
                copy_end: (new_pos + length as u64) as usize,
            },
        };
        on_match(m)?
    }
    Ok(())
}

pub fn diff_squashfs(
    old_path: &Path,
    old: &[u8],
    new_path: &Path,
    new: &[u8],
    out: &mut dyn Write,
    diff_params: &DiffParams,
) -> Result<(), io::Error> {
    let mut w = bidiff::enc::Writer::new(out)?;

    let mut translator = Translator::new(old, new, |control| w.write(control));
    // squashfs header with zstd takes 96 bytes
    diff(&old[0..96], &new[0..96], diff_params, |m| {
        translator.translate(m)
    })?;

    diff_squashfs_data(old_path, new_path, |m| {
        // println!("{:?}", m);
        translator.translate(m)
    })?;

    let footer_offset_old = get_inode_table_idx(old_path).unwrap();
    let footer_offset_new = get_inode_table_idx(new_path).unwrap();

    println!("footer_offset_old {}", footer_offset_old);
    println!("footer_offset_new {}", footer_offset_new);

    diff(
        &old[footer_offset_old..],
        &new[footer_offset_new..],
        diff_params,
        |m| {
            let m = Match {
                add_old_start: m.add_old_start + footer_offset_old,
                add_new_start: m.add_new_start + footer_offset_new,
                copy_end: m.copy_end + footer_offset_new,
                ..m
            };
            //        println!("{:?}", m);
            translator.translate(m)
        },
    )?;

    translator.close()?;

    Ok(())
}
