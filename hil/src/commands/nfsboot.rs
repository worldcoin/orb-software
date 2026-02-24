use camino::Utf8PathBuf;
use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use orb_s3_helpers::{ExistingFileBehavior, S3Uri};
use rand::SeedableRng;
use tracing::{debug, info};

use crate::nfsboot::{error_detection_for_host_state, request_sudo, MountSpec};

/// Boot the orb using NFS
#[derive(Debug, Parser)]
pub struct Nfsboot {
    /// The s3 URI of the RTS to use for NFS boot.
    #[arg(
        long,
        conflicts_with = "rts_path",
        required_unless_present = "rts_path"
    )]
    s3_url: Option<S3Uri>,
    /// The directory to save the s3 artifact we download.
    #[arg(long)]
    download_dir: Option<Utf8PathBuf>,
    /// Path to a downloaded RTS (zipped .tar or an already-extracted directory)
    /// used for NFS boot.
    #[arg(
        long,
        conflicts_with = "s3_url",
        conflicts_with = "download_dir",
        required_unless_present = "s3_url"
    )]
    rts_path: Option<Utf8PathBuf>,
    /// S3 URI of a separate RTS whose `rts/` directory is used for `--mount`
    /// content instead of the boot RTS. Use this when NFS-booting with a dev
    /// build but flashing a stage or prod build.
    #[arg(long, conflicts_with = "mount_rts_path")]
    mount_s3_url: Option<S3Uri>,
    /// Local path to a separate RTS tarball whose `rts/` directory is used for
    /// `--mount` content instead of the boot RTS.
    #[arg(long, conflicts_with = "mount_s3_url")]
    mount_rts_path: Option<Utf8PathBuf>,
    /// If this flag is given, overwites any existing files when downloading the rts.
    #[arg(long)]
    overwrite_existing: bool,
    /// Bind-mounts in the form <orb_mount_name>,<host_path>. Repeat --mount to add more.
    /// To mount the RTS itself, use `/rtsdir` as the host path (special case).
    #[arg(long = "mount")]
    mounts: Vec<MountSpec>,
    /// Path to directory containing persistent .img files to copy to bootloader dir
    #[arg(long)]
    persistent_img_path: Option<Utf8PathBuf>,
}

impl Nfsboot {
    pub async fn run(self) -> Result<()> {
        debug!("nfsboot called with args {self:?}");
        info!("In order to mount the rootfs, we need sudo");
        request_sudo().await?;
        error_detection_for_host_state()
            .await
            .wrap_err("failed to assert host's state")?;
        let rts_path = self.resolve_rts_path().await?;
        debug!("resolved boot RTS path: {rts_path}");

        let mount_rts_path = self.resolve_mount_rts_path().await?;
        if let Some(ref p) = mount_rts_path {
            debug!("resolved mount RTS path: {p}");
        }

        let rng = rand::rngs::StdRng::from_rng(rand::thread_rng()).unwrap();
        let _mount_guard = crate::nfsboot::nfsboot(
            rts_path,
            mount_rts_path,
            self.mounts,
            self.persistent_img_path.as_deref().map(|p| p.as_std_path()),
            rng,
        )
        .await
        .wrap_err("error while booting via nfs")?;

        info!("filesystems mounted, press ctrl-c to unmount and exit");
        std::future::pending::<()>().await;
        unreachable!()
    }

    fn existing_file_behavior(&self) -> ExistingFileBehavior {
        if self.overwrite_existing {
            ExistingFileBehavior::Overwrite
        } else {
            ExistingFileBehavior::Abort
        }
    }

    async fn resolve_rts_path(&self) -> Result<Utf8PathBuf> {
        let existing_file_behavior = self.existing_file_behavior();
        let rts_path = if let Some(ref s3_url) = self.s3_url {
            assert!(
                self.rts_path.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            let download_path = self
                .download_path_for_s3_url(s3_url)
                .wrap_err("failed to resolve boot rts download path")?;

            crate::download_s3::download_url(
                s3_url,
                &download_path,
                existing_file_behavior,
            )
            .await
            .wrap_err("error while downloading from s3")?;

            download_path
        } else if let Some(rts_path) = self.rts_path.clone() {
            assert!(
                self.s3_url.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            assert!(
                self.download_dir.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            info!("using already downloaded rts tarball");
            rts_path
        } else {
            bail!("you must provide either rts-path or s3-url");
        };

        Ok(rts_path)
    }

    async fn resolve_mount_rts_path(&self) -> Result<Option<Utf8PathBuf>> {
        let path = if let Some(ref s3_url) = self.mount_s3_url {
            assert!(
                self.mount_rts_path.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            let download_path = self
                .mount_download_path(s3_url)
                .wrap_err("failed to resolve mount rts download path")?;

            crate::download_s3::download_url(
                s3_url,
                &download_path,
                self.existing_file_behavior(),
            )
            .await
            .wrap_err("error while downloading mount rts from s3")?;

            Some(download_path)
        } else {
            self.mount_rts_path.clone()
        };

        Ok(path)
    }

    fn download_dir(&self) -> Utf8PathBuf {
        self.download_dir.clone().unwrap_or_else(crate::current_dir)
    }

    fn download_path_for_s3_url(&self, s3_url: &S3Uri) -> Result<Utf8PathBuf> {
        let file_name = crate::download_s3::parse_filename(s3_url)
            .wrap_err("failed to parse s3 filename")?;

        Ok(self.download_dir().join(file_name))
    }

    fn mount_download_path(&self, mount_s3_url: &S3Uri) -> Result<Utf8PathBuf> {
        let mount_download_path = self
            .download_path_for_s3_url(mount_s3_url)
            .wrap_err("failed to parse mount s3 filename")?;

        let collides_with_boot = if let Some(ref boot_s3_url) = self.s3_url {
            let boot_download_path = self
                .download_path_for_s3_url(boot_s3_url)
                .wrap_err("failed to parse boot s3 filename")?;
            mount_download_path == boot_download_path
        } else if let Some(ref rts_path) = self.rts_path {
            mount_download_path == *rts_path
        } else {
            false
        };

        if !collides_with_boot {
            return Ok(mount_download_path);
        }

        let mount_file_name = mount_download_path
            .file_name()
            .ok_or_else(|| color_eyre::eyre::eyre!("mount rts path has no filename"))?;

        Ok(self.download_dir().join(format!("mount-{mount_file_name}")))
    }
}

#[cfg(test)]
mod tests {
    use super::Nfsboot;
    use camino::Utf8PathBuf;
    use orb_s3_helpers::S3Uri;

    const DEV_S3: &str = "s3://worldcoin-orb-resources/worldcoin/orb-os/rts/2025-08-14-heads-main-0-g0a8d01b-diamond/rts-diamond-dev.tar.zstd";
    const STAGE_S3: &str = "s3://worldcoin-orb-resources/worldcoin/orb-os/rts/2025-08-14-heads-main-0-g0a8d01b-diamond/rts-diamond-stage.tar.zstd";

    const DEV_FILENAME: &str =
        "2025-08-14-heads-main-0-g0a8d01b-diamond-rts-diamond-dev.tar.zstd";
    const STAGE_FILENAME: &str =
        "2025-08-14-heads-main-0-g0a8d01b-diamond-rts-diamond-stage.tar.zstd";

    #[test]
    fn mount_download_path_disambiguated_when_both_s3_urls_collide() {
        let s3_url: S3Uri = DEV_S3.parse().unwrap();
        let command = Nfsboot {
            s3_url: Some(s3_url.clone()),
            download_dir: Some(Utf8PathBuf::from("/tmp/dl")),
            rts_path: None,
            mount_s3_url: Some(s3_url.clone()),
            mount_rts_path: None,
            overwrite_existing: false,
            mounts: vec![],
            persistent_img_path: None,
        };

        let path = command.mount_download_path(&s3_url).unwrap();
        assert_eq!(
            path,
            Utf8PathBuf::from(format!("/tmp/dl/mount-{DEV_FILENAME}"))
        );
    }

    #[test]
    fn mount_download_path_unchanged_when_s3_urls_differ() {
        let boot: S3Uri = DEV_S3.parse().unwrap();
        let mount: S3Uri = STAGE_S3.parse().unwrap();
        let command = Nfsboot {
            s3_url: Some(boot),
            download_dir: Some(Utf8PathBuf::from("/tmp/dl")),
            rts_path: None,
            mount_s3_url: Some(mount.clone()),
            mount_rts_path: None,
            overwrite_existing: false,
            mounts: vec![],
            persistent_img_path: None,
        };

        let path = command.mount_download_path(&mount).unwrap();
        assert_eq!(path, Utf8PathBuf::from(format!("/tmp/dl/{STAGE_FILENAME}")));
    }

    #[test]
    fn mount_download_path_disambiguated_when_collides_with_local_rts() {
        let mount: S3Uri = DEV_S3.parse().unwrap();
        let local_rts_path = Utf8PathBuf::from(format!("/tmp/dl/{DEV_FILENAME}"));
        let command = Nfsboot {
            s3_url: None,
            download_dir: Some(Utf8PathBuf::from("/tmp/dl")),
            rts_path: Some(local_rts_path),
            mount_s3_url: Some(mount.clone()),
            mount_rts_path: None,
            overwrite_existing: false,
            mounts: vec![],
            persistent_img_path: None,
        };

        let path = command.mount_download_path(&mount).unwrap();
        assert_eq!(
            path,
            Utf8PathBuf::from(format!("/tmp/dl/mount-{DEV_FILENAME}"))
        );
    }

    #[test]
    fn mount_download_path_unchanged_when_local_rts_differs() {
        let mount: S3Uri = STAGE_S3.parse().unwrap();
        let local_rts_path = Utf8PathBuf::from(format!("/tmp/dl/{DEV_FILENAME}"));
        let command = Nfsboot {
            s3_url: None,
            download_dir: Some(Utf8PathBuf::from("/tmp/dl")),
            rts_path: Some(local_rts_path),
            mount_s3_url: Some(mount.clone()),
            mount_rts_path: None,
            overwrite_existing: false,
            mounts: vec![],
            persistent_img_path: None,
        };

        let path = command.mount_download_path(&mount).unwrap();
        assert_eq!(path, Utf8PathBuf::from(format!("/tmp/dl/{STAGE_FILENAME}")));
    }
}
