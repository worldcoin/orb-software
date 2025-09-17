use cmd_lib::run_cmd;
use fs4::fs_std::FileExt;
use std::{
    fmt,
    fs::OpenOptions,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct QemuImg {
    base: String,
    steps: Vec<QemuStep>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum QemuStep {
    Write {
        guest_path: String,
        contents: String,
    },
    Package(String),
    Run(String),
}

impl fmt::Display for QemuStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use QemuStep::*;

        match self {
            Write {
                guest_path,
                contents,
            } => write!(f, "<write>{guest_path}:{contents}"),

            Package(pkg) => write!(f, "<package>{pkg}"),
            Run(cmd) => write!(f, "<run>{cmd}"),
        }
    }
}

impl QemuImg {
    pub fn base(&self) -> &str {
        &self.base
    }

    /// A base qcow2 image to build upon.
    pub fn from_base(guest_base: impl Into<String>) -> Self {
        Self {
            base: guest_base.into(),
            steps: vec![],
        }
    }

    pub fn to_hash(&self) -> String {
        let mut h = blake3::Hasher::new();
        let mut update = |tag: &str, contents: &str| {
            h.update(tag.as_bytes());
            h.update(&(contents.len() as u64).to_le_bytes());
            h.update(contents.as_bytes());
        };

        update("t", "qemuimgv1");
        update("b", &self.base);

        use QemuStep::*;
        for step in &self.steps {
            match step {
                Write {
                    guest_path,
                    contents,
                } => {
                    update("wp", guest_path);
                    update("wc", contents);
                }

                Package(pkg) => update("p", pkg),
                Run(cmd) => update("r", cmd),
            }
        }

        h.finalize().to_hex().to_string()
    }

    /// Ensures directory exists on guest, and writes to the filepath when image is being built.
    pub fn write(
        mut self,
        guest_path: impl Into<String>,
        contents: impl Into<String>,
    ) -> Self {
        self.steps.push(QemuStep::Write {
            guest_path: guest_path.into(),
            contents: contents.into(),
        });
        self
    }

    /// Installs a package on the guest using its package manager when image is being built.
    pub fn pkg(mut self, pkg: impl Into<String>) -> Self {
        self.steps.push(QemuStep::Package(pkg.into()));
        self
    }

    /// Installs a package on the guest using apt when image is being built.
    pub fn pkgs(mut self, pkgs: &[&str]) -> Self {
        for pkg in pkgs {
            self.steps.push(QemuStep::Package(pkg.to_string()));
        }

        self
    }

    /// Runs a command on the guest when image is being built.
    pub fn run(mut self, guest_cmd: impl Into<String>) -> Self {
        self.steps.push(QemuStep::Run(guest_cmd.into()));
        self
    }

    pub fn build(&self, working_dir: impl AsRef<Path>) -> PathBuf {
        let working_dir = working_dir.as_ref();
        let base_path = working_dir.join(&self.base).to_string_lossy().to_string();
        let hash = self.to_hash();
        let img = format!("{hash}.qcow2");
        let img_path = working_dir.join(img);

        let lock = format!("{hash}.lock");
        let lock_path = working_dir.join(lock);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .unwrap();

        file.lock_exclusive().unwrap();

        // if file exists by the time lock releases, it was most likely built by another thread or
        // process while lock was being held
        if img_path.exists() {
            return img_path;
        }

        run_cmd!(cp $base_path $img_path).unwrap();

        let mut cmd = Command::new("virt-customize");
        cmd.args([
            "-a",
            img_path.to_str().unwrap(),
            // dont bloat image with apt and logs
            "--run-command",
            "set -eux; \
    mkdir -p /var/lib/apt/lists /var/cache/apt/archives /var/log; \
    mount -t tmpfs tmpfs /var/lib/apt/lists; \
    mount -t tmpfs tmpfs /var/cache/apt/archives; \
    mount -t tmpfs tmpfs /var/log; \
    apt-get update",
        ]);

        for step in &self.steps {
            use QemuStep::*;
            match step {
                Write {
                    guest_path,
                    contents,
                } => {
                    cmd.args([
                        "--run-command",
                        &format!("mkdir -p \"$(dirname {guest_path})\""),
                    ]);

                    cmd.args([
                        "--run-command",
                        &format!("cat > {guest_path} <<'EOF'\n{contents}\nEOF"),
                    ]);
                }

                Package(pkg) => {
                    cmd.args(["--run-command", &format!("DEBIAN_FRONTEND=noninteractive apt-get -y install --no-install-recommends {pkg}")]);
                }

                Run(c) => {
                    cmd.args(["--run-command", c]);
                }
            }
        }

        cmd.args([
            "--run-command",
            "
    umount /var/lib/apt/lists; \
    umount /var/cache/apt/archives; \
    umount /var/log",
        ]);

        let output = cmd.output().expect("failed to spawn virt-customize");
        if !output.status.success() {
            let status = output.status.code().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);

            panic!("virt-customize failed with status: {status}, stderr: {stderr}");
        }

        img_path
    }
}
