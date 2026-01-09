use color_eyre::{eyre::eyre, Result};
use std::{pin::Pin, sync::Arc};
use tokio::task;
use zenoh::sample::Sample;

pub type Handler<Ctx> = Box<
    dyn Fn(Ctx, Sample) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>
        + Send
        + Sync,
>;

pub struct Receiver<'a, Ctx>
where
    Ctx: 'static + Clone + Send,
{
    orb_id: &'a str,
    env: &'a str,
    session: zenoh::Session,
    ctx: Ctx,
    subscribers: Vec<(&'static str, Handler<Ctx>)>,
}

impl<'a, Ctx> Receiver<'a, Ctx>
where
    Ctx: 'static + Clone + Send,
{
    pub(crate) fn new(
        env: &'a str,
        orb_id: &'a str,
        session: zenoh::Session,
        ctx: Ctx,
    ) -> Self {
        Self {
            env,
            orb_id,
            session,
            ctx,
            subscribers: Vec::new(),
        }
    }

    pub fn subscribe<H, Fut>(mut self, keyexpr: &'static str, handler: H) -> Self
    where
        H: Fn(Ctx, Sample) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<()>> + 'static + Send,
    {
        self.subscribers.push((
            keyexpr,
            Box::new(move |ctx, sample| Box::pin(handler(ctx, sample))),
        ));
        self
    }

    pub async fn run(self) -> Result<()> {
        for (keyexpr, handler) in self.subscribers {
            let subscriber = self
                .session
                .declare_subscriber(format!("{}/{}/{keyexpr}", self.env, self.orb_id))
                .await
                .map_err(|e| eyre!("{e}"))?;

            let ctx = self.ctx.clone();
            task::spawn(async move {
                loop {
                    let msg = match subscriber.recv_async().await {
                        Ok(msg) => msg,
                        Err(e) => {
                            tracing::error!(
                                "Failed to receive message for zenoh subscriber: '{}'. Terminating loop. Err {e}",
                                subscriber.key_expr()
                            );

                            break;
                        }
                    };

                    if let Err(e) = handler(ctx.clone(), msg).await {
                        tracing::error!(
                            "Handler for keyxpr '{}' faield with {e}",
                            subscriber.key_expr()
                        );
                    }
                }
            });
        }

        Ok(())
    }
}
