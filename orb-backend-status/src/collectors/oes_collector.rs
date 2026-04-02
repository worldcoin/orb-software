use super::ZenorbCtx;
use crate::orb_event_stream::Payload;
use color_eyre::Result;
use zenorb::zenoh::{self};

pub(crate) async fn handler(
    ctx: ZenorbCtx,
    sample: zenoh::sample::Sample,
) -> Result<()> {
    let payload = Payload::try_from(sample)?;
    ctx.oes.ingest(payload)?;

    Ok(())
}
