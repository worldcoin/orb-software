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
            let download_dir =
                self.download_dir.clone().unwrap_or_else(crate::current_dir);
            let download_path = download_dir.join(
                crate::download_s3::parse_filename(s3_url)
                    .wrap_err("failed to parse filename")?,
            );

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
            let download_dir =
                self.download_dir.clone().unwrap_or_else(crate::current_dir);
            let download_path = download_dir.join(
                crate::download_s3::parse_filename(s3_url)
                    .wrap_err("failed to parse mount rts filename")?,
            );

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
}
