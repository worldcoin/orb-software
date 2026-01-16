use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{eyre::Context, Result};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::{info, warn};

const ALLOWED_MOUNTPOINTS: &[&str] =
    &["/usr/persistent", "/mnt/updates", "/mnt/scratch"];

/// command format: `fsck ${device_path}`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let args = ctx.args();
    let device = if let Some(d) = args.first() {
        d
    } else {
        return Ok(ctx.failure().stderr("Missing device argument"));
    };

    info!("Running fsck on {} for job {}", device, ctx.execution_id());

    let input = match validate_fsck_arg(device) {
        Ok(input) => input,
        Err(e) => return Ok(ctx.failure().stderr(format!("{e}"))),
    };

    let mount_target = findmnt_target_for_source(&ctx, device).await;
    let mount_target_opt = mount_target.ok();
    let (fsck_target, remount_target) =
        match findmnt_source_for_target(&ctx, device).await {
            // User passed a mountpoint (e.g. /usr/persistent). Fsck the backing block
            // device and remount the mountpoint afterwards.
            Ok(source) => (source, Some(device.to_string())),
            // Not a mountpoint. Fsck the provided arg (file/device), but remount if we
            // discovered it's mounted somewhere.
            Err(_) => (device.to_string(), mount_target_opt.clone()),
        };

    // Enforce allowlist after we learned whether this is a mountpoint or a device.
    // - Mountpoints: only a small allowlist
    // - Devices: only if they resolve to an allowed mountpoint
    match input {
        FsckArg::Mountpoint(target) => {
            if !ALLOWED_MOUNTPOINTS.contains(&target.as_str()) {
                return Ok(ctx.failure().stderr(format!(
                    "Refusing to run fsck on mountpoint {target}; allowed mountpoints: {}",
                    ALLOWED_MOUNTPOINTS.join(", ")
                )));
            }
        }
        FsckArg::Device(path) => {
            if let Some(target) = mount_target_opt.clone()
                && ALLOWED_MOUNTPOINTS.contains(&target.as_str())
            {
                // Allowed.
            } else {
                return Ok(ctx.failure().stderr(format!(
                    "Refusing to run fsck on device {path}. Please pass an allowed mountpoint instead: {}",
                    ALLOWED_MOUNTPOINTS.join(", ")
                )));
            }
        }
        #[cfg(any(test, feature = "integration-test"))]
        FsckArg::TestFile(_path) => {}
    }

    if let Some(target) = &remount_target {
        let unmount = ctx
            .deps()
            .shell
            .exec(&["umount", target])
            .await
            .context("failed to spawn umount")?
            .wait_with_output()
            .await
            .context("failed to wait for umount")?;

        if !unmount.status.success() {
            let stdout = String::from_utf8_lossy(&unmount.stdout);
            let stderr = String::from_utf8_lossy(&unmount.stderr);
            let message = format!(
                "Refusing to run fsck: target appears mounted at {target} and unmount failed.\n\nUMOUNT STDOUT:\n{stdout}\nUMOUNT STDERR:\n{stderr}"
            );
            return Ok(ctx.failure().stderr(message));
        }
    }

    // Verify filesystem type before running fsck.
    let fs_type = blkid_fs_type(&ctx, &fsck_target).await;
    let fs_type = match fs_type {
        Ok(t) => t,
        Err(e) => {
            return Ok(ctx.failure().stderr(format!(
                "Refusing to run fsck on {fsck_target}: could not determine filesystem type via blkid: {e}"
            )));
        }
    };
    if !is_allowed_fs_type(&fs_type) {
        return Ok(ctx.failure().stderr(format!(
            "Refusing to run fsck on {fsck_target}: filesystem type {fs_type} is not allowed"
        )));
    }

    let output = ctx
        .deps()
        .shell
        .exec(&["fsck", "-y", "-f", &fsck_target])
        .await
        .context("failed to spawn fsck")?
        .wait_with_output()
        .await
        .context("failed to wait for fsck")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let fsck_message = format!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");

    let mut remount_message = String::new();
    let mut remount_ok = true;
    if let Some(target) = &remount_target {
        // Best-effort remount: prefer fstab-based `mount <target>`, then fall back to
        // `mount <source> <target>`.
        let mount1 = ctx
            .deps()
            .shell
            .exec(&["mount", target])
            .await
            .context("failed to spawn mount")?
            .wait_with_output()
            .await
            .context("failed to wait for mount")?;

        let mut ok = mount1.status.success();
        if !ok {
            let mount2 = ctx
                .deps()
                .shell
                .exec(&["mount", &fsck_target, target])
                .await
                .context("failed to spawn mount (fallback)")?
                .wait_with_output()
                .await
                .context("failed to wait for mount (fallback)")?;
            ok = mount2.status.success();
        }
        remount_ok = ok;

        // Even if remount fails, surface that in job output
        match findmnt_source_for_target(&ctx, target).await {
            Ok(source) => {
                remount_message = format!("\n\nRemount: OK ({target} -> {source})");
            }
            Err(e) => {
                warn!("failed to confirm remount of {target}: {e:?}");
                remount_message = format!("\n\nRemount: FAILED ({target})");
            }
        }
    }

    let message = format!("{fsck_message}{remount_message}");

    // fsck exit codes:
    // 0 - No errors
    // 1 - File system errors corrected
    // 2 - System should be rebooted
    // ...
    if let Some(code) = output.status.code()
        && (code == 0 || code == 1)
    {
        if remount_ok {
            return Ok(ctx.success().stdout(message));
        }

        return Ok(ctx.failure().stdout(message).stderr("Remount failed"));
    }

    Ok(ctx.failure().stdout(message))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FsckArg {
    Mountpoint(String),
    Device(String),
    #[cfg(any(test, feature = "integration-test"))]
    TestFile(String),
}

fn validate_fsck_arg(arg: &str) -> Result<FsckArg> {
    if ALLOWED_MOUNTPOINTS.contains(&arg) {
        return Ok(FsckArg::Mountpoint(arg.to_string()));
    }

    if arg.starts_with("/dev/") {
        return Ok(FsckArg::Device(arg.to_string()));
    }

    #[cfg(any(test, feature = "integration-test"))]
    {
        Ok(FsckArg::TestFile(arg.to_string()))
    }

    #[cfg(not(any(test, feature = "integration-test")))]
    {
        Err(color_eyre::eyre::eyre!(
            "Refusing to run fsck on {arg}; allowed mountpoints: {}",
            ALLOWED_MOUNTPOINTS.join(", ")
        ))
    }
}

fn is_allowed_fs_type(fs_type: &str) -> bool {
    matches!(fs_type, "ext2" | "ext3" | "ext4" | "f2fs")
}

async fn blkid_fs_type(ctx: &Ctx, device: &str) -> Result<String> {
    let output = ctx
        .deps()
        .shell
        .exec(&["blkid", "-o", "value", "-s", "TYPE", device])
        .await
        .context("failed to spawn blkid")?
        .wait_with_output()
        .await
        .context("failed to wait for blkid")?;

    if !output.status.success() {
        return Err(color_eyre::eyre::eyre!(
            "blkid failed with status {}",
            output.status
        ));
    }

    let t = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if t.is_empty() {
        return Err(color_eyre::eyre::eyre!("blkid returned empty TYPE"));
    }

    Ok(t)
}

async fn findmnt_source_for_target(ctx: &Ctx, target: &str) -> Result<String> {
    let output = ctx
        .deps()
        .shell
        .exec(&["findmnt", "-n", "-o", "SOURCE", "--target", target])
        .await
        .context("failed to spawn findmnt")?
        .wait_with_output()
        .await
        .context("failed to wait for findmnt")?;

    if !output.status.success() {
        return Err(color_eyre::eyre::eyre!(
            "findmnt failed with status {}",
            output.status
        ));
    }

    let source = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if source.is_empty() {
        return Err(color_eyre::eyre::eyre!("findmnt returned empty SOURCE"));
    }

    Ok(source)
}

async fn findmnt_target_for_source(ctx: &Ctx, source: &str) -> Result<String> {
    let output = ctx
        .deps()
        .shell
        .exec(&["findmnt", "-n", "-o", "TARGET", "--source", source])
        .await
        .context("failed to spawn findmnt")?
        .wait_with_output()
        .await
        .context("failed to wait for findmnt")?;

    if !output.status.success() {
        return Err(color_eyre::eyre::eyre!(
            "findmnt failed with status {}",
            output.status
        ));
    }

    let target = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if target.is_empty() {
        return Err(color_eyre::eyre::eyre!("findmnt returned empty TARGET"));
    }

    Ok(target)
}
