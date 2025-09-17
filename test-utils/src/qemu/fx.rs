use super::{img::QemuImg, instance::QemuInstance};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn run(workdir: impl AsRef<Path>, img: &QemuImg) -> QemuInstance {
    if !s3::is_authed() {
        panic!("\nplease authenticate with s3 before continuing\n");
    }

    let img_path = get_or_build_img(&workdir, img);
    QemuInstance::start(workdir, img_path)
}

fn get_or_build_img(workdir: impl AsRef<Path>, qemu_img: &QemuImg) -> PathBuf {
    let workdir = workdir.as_ref();
    let workdir_str = workdir.to_str().unwrap();

    let img = format!("{}.qcow2", qemu_img.to_hash());

    let img_workdir_path = workdir.join(&img);
    println!("checking for QemuImg in workdir: {img_workdir_path:?}");

    if fs::exists(&img_workdir_path).unwrap() {
        return img_workdir_path;
    }

    println!(
        "QemuImg does not exist. Looking for it in {}",
        s3::VM_S3_PATH
    );

    if s3::get_vm(workdir_str, &img) {
        return img_workdir_path;
    }

    println!("QemuImg does not exist in S3, will build locally.");

    let base_img_path = workdir.join(qemu_img.base());
    println!("Looking for base image in workdir: {base_img_path:?}");

    if !fs::exists(&base_img_path).unwrap() {
        println!("Base image not found locally. Pulling from s3.");
        if !s3::get_vm(workdir_str, qemu_img.base()) {
            panic!(
                "Could not find base image {} on S3. Nothing else to do.",
                qemu_img.base()
            );
        }
    }

    println!("Building QemuImg.");
    qemu_img.build(workdir)
}

mod s3 {
    use std::{fs::OpenOptions, path::Path};

    use cmd_lib::run_cmd;
    use fs4::fs_std::FileExt;

    pub const VM_S3_PATH: &str = "s3://worldcoin-orb-resources/virtual-machines";

    pub fn is_authed() -> bool {
        run_cmd!(aws sts get-caller-identity).is_ok()
    }

    pub fn get_vm(workdir: impl AsRef<Path>, filename: &str) -> bool {
        let workdir = workdir.as_ref();
        let workdir_str = workdir.to_str().unwrap();

        let lock_path = workdir.join(format!("{filename}.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .unwrap();

        file.lock_exclusive().unwrap();

        if workdir.join(filename).exists() {
            return true;
        }

        run_cmd!(aws s3 cp $VM_S3_PATH/$filename $workdir_str).is_ok()
    }

    pub fn upload_vm(workdir: &str, filename: &str) {
        run_cmd!(aws s3 cp $workdir/$filename $VM_S3_PATH).unwrap();
    }
}
