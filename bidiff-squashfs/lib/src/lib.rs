use std::{
    collections::HashMap,
    ffi::{c_char, CString},
    io::{self, Write},
    path::Path,
};

pub mod reexports {
    pub use bidiff;
}

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

#[bon::builder]
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

#[cfg(test)]
mod test {
    use color_eyre::eyre::WrapErr as _;
    use color_eyre::Result;
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Cursor, Write};

    use super::*;

    #[bon::builder]
    fn do_bipatch(
        base: impl io::Read + io::Seek,
        patch: impl io::Read,
        mut out: impl io::Write,
    ) -> Result<u64> {
        let mut processor =
            bipatch::Reader::new(patch, base).wrap_err("failed to decode patch")?;
        io::copy(&mut processor, &mut out).wrap_err("failed to process patch")
    }

    fn sqfs_from_dir(from_dir: &Path, to_file: &Path) -> Result<()> {
        cmd_lib::run_cmd!(mksquashfs $from_dir $to_file -comp zstd > /dev/null)
            .wrap_err("failed to run mksquashfs")
    }

    fn sqfs_from_files(
        out_path: &Path,
        files: impl Iterator<Item = (impl AsRef<Path>, impl AsRef<[u8]>)>,
    ) -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        for (filename, file_contents) in files {
            let filename = filename.as_ref();
            assert!(filename.is_relative(), "filename should only be relative");
            let path = tmp_dir.path().join(filename);
            let mut file = fs::File::create_new(&path)
                .wrap_err_with(|| format!("failed to create {}", path.display()))?;
            file.write_all(file_contents.as_ref()).wrap_err_with(|| {
                format!("failed to populate contents of {}", path.display())
            })?;
            file.flush()?;
        }

        sqfs_from_dir(tmp_dir.path(), out_path)?;

        Ok(())
    }

    fn test_examples(
        examples: impl IntoIterator<
            Item = (impl AsRef<str>, HashMap<impl AsRef<Path>, Vec<u8>>),
        >,
    ) -> Result<()> {
        let tmp_dir = tempfile::tempdir().unwrap();
        println!("starting diffing test in dir: {}", tmp_dir.path().display());
        let examples: Result<Vec<_>> = examples
            .into_iter()
            .map(|(file_name, contents)| {
                let sqfs_path = tmp_dir
                    .path()
                    .join(file_name.as_ref())
                    .with_extension("sqfs");
                sqfs_from_files(&sqfs_path, contents.iter())?;
                Ok((sqfs_path, contents))
            })
            .collect();
        let examples = examples?;

        for (sqfs_path, _sqfs_contents) in examples {
            let sqfs_bytes = fs::read(&sqfs_path).wrap_err_with(|| {
                format!("failed to read sqfs contents at {}", sqfs_path.display())
            })?;
            let patch_path = sqfs_path.with_extension("patch");
            let mut patch_file = fs::File::create_new(&patch_path)
                .wrap_err("failed to create empty patch file")?;

            println!("before diff: {sqfs_path:?}");
            diff_squashfs()
                .old_path(&sqfs_path)
                .old(&sqfs_bytes)
                .new_path(&sqfs_path)
                .new(&sqfs_bytes)
                .out(&mut patch_file)
                .diff_params(&DiffParams::default())
                .call()
                .wrap_err("failed to perform diff")?;
            patch_file.flush().wrap_err("failed to flush patch file")?;
            drop(patch_file); // we are going to reopen as read-only

            println!("before bipatch: {sqfs_path:?}");
            let after_patch_path = sqfs_path.with_extension("after_patch.sqfs");
            let mut after_patch_file = fs::File::create_new(&after_patch_path)
                .wrap_err("failed to create empty after_patch file")?;
            let patch_file = fs::File::open(&patch_path)
                .wrap_err("failed to open populated patch file")?;
            let n_bytes = do_bipatch()
                .base(Cursor::new(&sqfs_bytes))
                .patch(&patch_file)
                .out(&mut after_patch_file)
                .call()
                .wrap_err("failed to apply patch")?;
            assert_eq!(
                n_bytes,
                after_patch_path.metadata().expect("failed metadata").len(),
                "n_bytes returned by do_bipatch didn't match file contents"
            );

            assert_eq!(
                std::fs::read(after_patch_path).unwrap(),
                sqfs_bytes,
                "post-patch results should match the original file because\
                nothing was supposed to change"
            );
        }

        Ok(())
    }

    #[test]
    fn test_diffing_self_then_patching_produces_same_output() -> Result<()> {
        let examples = [
            ("1BFile", HashMap::from([("1BFile", vec![69])])),
            (
                "two_1BFiles",
                HashMap::from([("1BFile1", vec![69]), ("1BFile2", vec![69])]),
            ),
            ("1KiBFile", HashMap::from([("1KiBFile", vec![69; 1024])])),
            (
                "two_1KiBFiles",
                HashMap::from([
                    ("1KiBFile1", vec![69; 1024]),
                    ("1KiBFile2", vec![69; 1024]),
                ]),
            ),
            (
                "1MiBFile",
                HashMap::from([("1MiBFile", vec![69; 1024 * 1024])]),
            ),
            (
                "two_1MiBFiles",
                HashMap::from([
                    ("1MiBFile1", vec![69; 1024 * 1024]),
                    ("1MiBFile2", vec![69; 1024 * 1024]),
                ]),
            ),
        ];
        test_examples(examples)
    }

    #[test]
    fn test_diffing_empty_self_then_patching_produces_same_output() -> Result<()> {
        let examples = [
            ("empty", HashMap::new()),
            ("single_empty_file", HashMap::from([("empty", Vec::new())])),
            (
                "two_empty_files",
                HashMap::from([("empty1", Vec::new()), ("empty2", Vec::new())]),
            ),
        ];
        test_examples(examples)
    }
}
