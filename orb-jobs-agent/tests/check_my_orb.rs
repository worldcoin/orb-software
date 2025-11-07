use common::{fake_orb::FakeOrb, fixture::JobAgentFixture};
use tokio::fs;

mod common;

// No docker in macos on github
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_executes_check_my_orb() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    fx.program().shell(FakeOrb::new().await).spawn().await;

    // Act
    fx.enqueue_job("check_my_orb")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let actual = fx
        .execution_updates
        .read()
        .await
        .first()
        .unwrap()
        .std_out
        .clone();

    let expected =
        fs::read_to_string(FakeOrb::context_dir().join("check-my-orb_output.txt"))
            .await
            .unwrap();

    assert_eq!(actual, expected);
}
