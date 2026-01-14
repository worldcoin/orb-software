use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::Result;
use orb_speed_test::{run_pcp_speed_test, run_speed_test};

use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use tracing::info;

const TEST_SIZE_BYTES: usize = 20_000_000;
const NUMBER_OF_PCP_UPLOADS: usize = 3;

#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let orb_id = &ctx.deps().settings.orb_id;
    let dbus_connection = &ctx.deps().session_dbus;

    info!("Running PCP speed test");
    let pcp_result = run_pcp_speed_test(
        TEST_SIZE_BYTES,
        orb_id,
        dbus_connection,
        NUMBER_OF_PCP_UPLOADS,
    )
    .await?;

    info!("Running regular speed test");

    let basic_result = run_speed_test(TEST_SIZE_BYTES).await?;

    let result = serde_json::json!({
        "pcp_speed_test": pcp_result,
        "internet_speed_test": basic_result
    });

    Ok(ctx.success().stdout(result.to_string()))
}
