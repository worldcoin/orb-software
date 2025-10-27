use std::ffi::OsStr;
use std::path::Path;
use std::process::Output;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::task;

pub async fn build(
    tag: impl AsRef<OsStr>,
    dockerfile: impl AsRef<Path>,
    context: impl AsRef<Path>,
) -> Output {
    tokio::process::Command::new("docker")
        .arg("build")
        .arg("-t")
        .arg(tag)
        .arg("-f")
        .arg(dockerfile.as_ref().to_str().unwrap())
        .arg(context.as_ref().to_str().unwrap())
        .output()
        .await
        .unwrap()
}

/// Starts a container with a temporary directory mounted to /run/integration-tests
pub async fn run<I, S>(img: impl AsRef<OsStr>, args: I) -> Container
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let tempdir = TempDir::new_in("/tmp").unwrap();
    let tempdir_path = tempdir.path().canonicalize().unwrap();

    let out = Command::new("docker")
        .args(["run", "-d", "--rm"])
        .args([
            "-v",
            &format!("{}:/run/integration-tests", tempdir_path.display()),
        ])
        .args(args)
        .arg(img)
        .output()
        .await
        .unwrap();

    if !out.status.success() {
        panic!("{}", String::from_utf8_lossy(&out.stderr));
    }

    Container {
        id: String::from_utf8(out.stdout).unwrap(),
        tempdir,
    }
}

pub struct Container {
    pub id: String,
    pub tempdir: TempDir,
}

impl Drop for Container {
    fn drop(&mut self) {
        let cid = self.id.clone();

        task::spawn(async move {
            Command::new("docker")
                .args(["rm", "-f", &cid]) // force stop + remove
                .output()
                .await
                .unwrap();
        });
    }
}
