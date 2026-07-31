use std::{
    fs::{self, OpenOptions},
    io::{Seek, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use flate2::{write::GzEncoder, Compression};

const SALT: &str = "4a6f75726e6579206265666f72652064657374696e6174696f6e";
const UUID: &str = "54686520-5768-6565-6c20-776561766573";
const DATA_BLOCKS: u64 = 1;
const HASH_OFFSET: u64 = 4_096;

#[derive(Clone, Copy)]
struct VerityImage {
    label: &'static str,
    contents: &'static str,
}

const DIAMOND_IMAGE: VerityImage = VerityImage {
    label: "SYSTEM",
    contents: "abracadabra",
};

const PEARL_IMAGES: [VerityImage; 9] = [
    VerityImage {
        label: "AI_LAYER",
        contents: "hocus-pocus",
    },
    VerityImage {
        label: "BASE_LAYER",
        contents: "open-sesame",
    },
    VerityImage {
        label: "CACHE_LAYER",
        contents: "shazam",
    },
    VerityImage {
        label: "CUDA_LAYER",
        contents: "presto",
    },
    VerityImage {
        label: "LFT_LAYER",
        contents: "alakazam",
    },
    VerityImage {
        label: "PACKAGES_LAYER",
        contents: "simsalabim",
    },
    VerityImage {
        label: "SECURITY_LAYER",
        contents: "hex",
    },
    VerityImage {
        label: "SOFTWARE_LAYER",
        contents: "tadaa",
    },
    VerityImage {
        label: "SYSTEM_LAYER",
        contents: "voila",
    },
];

fn format_verity_image(image: &Path, contents: &str) -> String {
    let root_hash = image.with_extension("root-hash");
    fs::write(image, contents).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(image)
        .unwrap()
        .set_len(HASH_OFFSET)
        .unwrap();

    assert!(Command::new("veritysetup")
        .args([
            "--format=1",
            "--hash=sha256",
            "--data-block-size=4096",
            "--hash-block-size=4096",
        ])
        .arg(format!("--data-blocks={DATA_BLOCKS}"))
        .arg(format!("--hash-offset={HASH_OFFSET}"))
        .arg(format!("--salt={SALT}"))
        .arg(format!("--uuid={UUID}"))
        .arg("--root-hash-file")
        .arg(&root_hash)
        .args(["format"])
        .arg(image)
        .arg(image)
        .status()
        .unwrap()
        .success());

    fs::read_to_string(root_hash).unwrap().trim().to_owned()
}

fn corrupt_image(image: &Path) {
    let mut image = OpenOptions::new().write(true).open(image).unwrap();
    image.seek(std::io::SeekFrom::Start(0)).unwrap();
    image.write_all(b"corrupted").unwrap();
}

pub struct DiamondFixture {
    pub source_root: PathBuf,
    _tempdir: tempfile::TempDir,
}

impl DiamondFixture {
    // The fixtures are not a public API of a crate,
    // but just test specific infrastructure.
    // Might evolve to take a `config` as constructing
    // metadata instead of reading const values
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let fixture = tempfile::tempdir().unwrap();
        let cmdline = fixture.path().join("proc/cmdline");
        let image = fixture
            .path()
            .join("dev/disk/by-partlabel")
            .join(DIAMOND_IMAGE.label);
        fs::create_dir_all(cmdline.parent().unwrap()).unwrap();
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        let root_hash = format_verity_image(&image, DIAMOND_IMAGE.contents);
        fs::write(
            cmdline,
            format!(
                "VERITY_ROOT_HASH={} VERITY_DATA_BLOCKS={} VERITY_HASH_OFFSET={}",
                root_hash, DATA_BLOCKS, HASH_OFFSET,
            ),
        )
        .unwrap();

        Self {
            source_root: fixture.path().to_owned(),
            _tempdir: fixture,
        }
    }

    pub fn corrupt(self) -> Self {
        corrupt_image(&self.source_root.join("dev/disk/by-partlabel/SYSTEM"));
        self
    }
}

pub struct PearlFixture {
    pub source_root: PathBuf,
    _tempdir: tempfile::TempDir,
}

impl PearlFixture {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let fixture = tempfile::tempdir().unwrap();
        let images = fixture.path().join("dev/disk/by-partlabel");
        fs::create_dir_all(&images).unwrap();

        let mut variables = String::new();
        for image in PEARL_IMAGES {
            let layer = image.label.strip_suffix("_LAYER").unwrap();
            let root_hash =
                format_verity_image(&images.join(image.label), image.contents);
            variables.push_str(&format!(
                "export {layer}_VERITY_HASH='{}'\nexport {layer}_DATA_BLOCKS='{}'\nexport {layer}_HASH_OFFSET='{}'\n",
                root_hash,
                DATA_BLOCKS,
                HASH_OFFSET,
            ));
        }

        let variables_path = fixture.path().join("verity_variables.env");
        fs::write(&variables_path, variables).unwrap();
        let mut cpio = Command::new("cpio")
            .current_dir(fixture.path())
            .args(["--quiet", "--create", "--format=newc"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        cpio.stdin
            .take()
            .unwrap()
            .write_all(b"verity_variables.env\n")
            .unwrap();
        let cpio = cpio.wait_with_output().unwrap();
        assert!(cpio.status.success());

        let initrd_path = fixture.path().join("initrd");
        let mut initrd = GzEncoder::new(
            fs::File::create(&initrd_path).unwrap(),
            Compression::default(),
        );
        initrd.write_all(&cpio.stdout).unwrap();
        initrd.finish().unwrap();

        let app_image = images.join("APP");
        fs::File::create(&app_image)
            .unwrap()
            .set_len(1_048_576)
            .unwrap();
        assert!(Command::new("mkfs.vfat")
            .args(["-F", "12", "-i", "12345678"])
            .arg(&app_image)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("mmd")
            .args(["-i"])
            .arg(&app_image)
            .arg("::app")
            .status()
            .unwrap()
            .success());
        assert!(Command::new("mmd")
            .args(["-i"])
            .arg(&app_image)
            .arg("::app/boot")
            .status()
            .unwrap()
            .success());
        assert!(Command::new("mcopy")
            .args(["-i"])
            .arg(&app_image)
            .arg(&initrd_path)
            .arg("::app/boot/initrd")
            .status()
            .unwrap()
            .success());

        Self {
            source_root: fixture.path().to_owned(),
            _tempdir: fixture,
        }
    }

    pub fn corrupt_system_image(self) -> Self {
        corrupt_image(&self.source_root.join("dev/disk/by-partlabel/SYSTEM_LAYER"));
        self
    }
}
