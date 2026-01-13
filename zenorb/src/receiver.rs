use color_eyre::{eyre::eyre, Result};
use orb_info::{orb_os_release::OrbRelease, OrbId};
use std::pin::Pin;
use tokio::task::{self, JoinHandle};
use zenoh::{query::Query, sample::Sample};

pub type Handler<Ctx, Payload> = Box<
    dyn Fn(Ctx, Payload) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>
        + Send
        + Sync,
>;

pub struct Receiver<'a, Ctx>
where
    Ctx: 'static + Clone + Send,
{
    orb_id: &'a str,
    env: &'a str,
    service_name: &'a str,
    session: zenoh::Session,
    ctx: Ctx,
    subscribers: Vec<(&'static str, Handler<Ctx, Sample>)>,
    queryables: Vec<(&'static str, Handler<Ctx, Query>)>,
}

impl<'a, Ctx> Receiver<'a, Ctx>
where
    Ctx: 'static + Clone + Send,
{
    pub(crate) fn new(
        env: &'a OrbRelease,
        orb_id: &'a OrbId,
        service_name: &'a str,
        session: zenoh::Session,
        ctx: Ctx,
    ) -> Self {
        Self {
            env: env.as_str(),
            orb_id: orb_id.as_str(),
            service_name,
            session,
            ctx,
            subscribers: Vec::new(),
            queryables: Vec::new(),
        }
    }

    pub fn subscriber<H, Fut>(mut self, keyexpr: &'static str, handler: H) -> Self
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

    pub fn queryable<H, Fut>(mut self, keyexpr: &'static str, handler: H) -> Self
    where
        H: Fn(Ctx, Query) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<()>> + 'static + Send,
    {
        self.queryables.push((
            keyexpr,
            Box::new(move |ctx, sample| Box::pin(handler(ctx, sample))),
        ));
        self
    }

    pub async fn run(self) -> Result<Vec<JoinHandle<()>>> {
        let mut tasks = Vec::new();

        for (keyexpr, handler) in self.subscribers {
            let subscriber = self
                .session
                .declare_subscriber(format!("{}/{}/{keyexpr}", self.env, self.orb_id))
                .await
                .map_err(|e| eyre!("{e}"))?;

            let ctx = self.ctx.clone();
            let handle = task::spawn(async move {
                loop {
                    let sample = match subscriber.recv_async().await {
                        Ok(sample) => sample,
                        Err(e) => {
                            tracing::error!(
                                "Failed to receive message for zenoh subscriber: '{}'. Terminating loop. Err {e}",
                                subscriber.key_expr()
                            );

                            break;
                        }
                    };

                    if let Err(e) = handler(ctx.clone(), sample).await {
                        tracing::error!(
                            "Subscriber for keyexpr '{}' failed with {e}",
                            subscriber.key_expr()
                        );
                    }
                }
            });

            tasks.push(handle);
        }

        for (keyexpr, handler) in self.queryables {
            let queryable = self
                .session
                .declare_queryable(format!(
                    "{}/{}/{}/{keyexpr}",
                    self.env, self.orb_id, self.service_name
                ))
                .await
                .map_err(|e| eyre!("{e}"))?;

            let ctx = self.ctx.clone();
            let handle = task::spawn(async move {
                loop {
                    let query = match queryable.recv_async().await {
                        Ok(query) => query,
                        Err(e) => {
                            tracing::error!(
                                "Failed to receive message for zenoh queryable: '{}'. Terminating loop. Err {e}",
                                queryable.key_expr()
                            );

                            break;
                        }
                    };

                    if let Err(e) = handler(ctx.clone(), query).await {
                        tracing::error!(
                            "Queryable for keyexpr '{}' failed with {e}",
                            queryable.key_expr()
                        );
                    }
                }
            });

            tasks.push(handle);
        }

        Ok(tasks)
    }
}
