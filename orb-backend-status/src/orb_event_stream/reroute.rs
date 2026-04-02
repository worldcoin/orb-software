use crate::{collectors::ZenorbCtx, orb_event_stream::Payload};
use std::time::Duration;
use zenorb::Receiver;

pub(crate) trait OesReroute {
    fn oes_reroute(
        self,
        keyexpr: impl Into<String>,
        query_timeout: Duration,
        mode: oes::Mode,
    ) -> Self;
}

impl<'a> OesReroute for Receiver<'a, ZenorbCtx> {
    fn oes_reroute(
        self,
        keyexpr: impl Into<String>,
        query_timeout: Duration,
        mode: oes::Mode,
    ) -> Self {
        self.querying_subscriber(
            keyexpr,
            query_timeout,
            move |ctx, sample| async move {
                let mut payload = Payload::try_from(sample)?;
                payload.headers.mode = mode;
                ctx.oes.ingest(payload)?;

                Ok(())
            },
        )
    }
}
