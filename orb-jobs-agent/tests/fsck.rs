use color_eyre::Result;
use common::{fake_orb::FakeOrb, fixture::JobAgentFixture};
use orb_jobs_agent::shell::Shell;
use orb_relay_messages::jobs::v1::JobExecutionStatus;

mod common;

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn fsck_real_clean_image() {
    let fx = JobAgentFixture::new().await;
    let orb = FakeOrb::new().await;
    let image_path = "/tmp/clean.img";

    let status = orb
        .exec(&[
            "dd",
            "if=/dev/zero",
            &format!("of={image_path}"),
            "bs=1M",
            "count=32",
        ])
        .await
        .expect("failed to spawn dd")
        .wait()
        .await
        .expect("failed to wait for dd");
    assert!(status.success(), "dd failed");

    let status = orb
        .exec(&["mkfs.ext4", "-F", image_path])
        .await
        .expect("failed to spawn mkfs")
        .wait()
        .await
        .expect("failed to wait for mkfs");
    assert!(status.success(), "mkfs failed");

    fx.program().shell(orb).spawn().await;

    fx.enqueue_job(format!("fsck {image_path}"))
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    assert_eq!(
        result.status,
        JobExecutionStatus::Succeeded as i32,
        "fsck should succeed on a clean image"
    );
    assert!(
        result.std_out.contains("clean"),
        "Output should indicate filesystem is clean. Got: {}",
        result.std_out
    );
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn fsck_real_corrupted_image() {
    let fx = JobAgentFixture::new().await;
    let orb = FakeOrb::new().await;
    let image_path = "/tmp/corrupt.img";

    let status = orb
        .exec(&[
            "dd",
            "if=/dev/zero",
            &format!("of={image_path}"),
            "bs=1M",
            "count=32",
        ])
        .await
        .expect("failed to spawn dd")
        .wait()
        .await
        .expect("failed to wait for dd");
    assert!(status.success(), "dd failed");

    let status = orb
        .exec(&["mkfs.ext4", "-F", image_path])
        .await
        .expect("failed to spawn mkfs")
        .wait()
        .await
        .expect("failed to wait for mkfs");
    assert!(status.success(), "mkfs failed");

    // Corrupt the filesystem (write garbage to the middle)
    let status = orb
        .exec(&[
            "dd",
            "if=/dev/urandom",
            &format!("of={image_path}"),
            "bs=4k",
            "count=10",
            "seek=1000",
            "conv=notrunc",
        ])
        .await
        .expect("failed to spawn corruption dd")
        .wait()
        .await
        .expect("failed to wait for corruption dd");
    assert!(status.success(), "corruption dd failed");

    fx.program().shell(orb).spawn().await;

    fx.enqueue_job(format!("fsck {image_path}"))
        .await
        .wait_for_completion()
        .await;

    // 6. Verify result
    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    // We expect fsck -y to be able to repair minor corruption (inode table/data blocks).
    // It returns 1 (File system errors corrected) which our handler maps to Success.
    // If it returns 0 (No errors), that's also Success (maybe we hit unused blocks).
    assert_eq!(
        result.status,
        JobExecutionStatus::Succeeded as i32,
        "fsck -y should have fixed the corruption. Output: STDOUT:\n{}\nSTDERR:\n{}",
        result.std_out,
        result.std_err
    );
}

#[tokio::test]
async fn fsck_fails_missing_arg_unit() {
    #[derive(Clone, Debug)]
    struct UnitShell;
    #[async_trait::async_trait]
    impl Shell for UnitShell {
        async fn exec(&self, _cmd: &[&str]) -> Result<tokio::process::Child> {
            unreachable!("Should not be called");
        }
    }

    let fx = JobAgentFixture::new().await;
    fx.program().shell(UnitShell).spawn().await;

    fx.enqueue_job("fsck").await.wait_for_completion().await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();
    assert_eq!(result.status, JobExecutionStatus::Failed as i32);
    assert!(result.std_err.contains("Missing device argument"));
}
