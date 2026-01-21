use color_eyre::{eyre::eyre, Result};
use orb_info::OrbId;
use std::{pin::Pin, time::Duration};
use tokio::{
    task::{self, JoinHandle},
    time::{self, Instant},
};
use zenoh::{query::Query, sample::Sample};

pub type Callback<Ctx, Payload> = Box<
    dyn Fn(Ctx, Payload) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>
        + Send
        + Sync,
>;

pub struct Receiver<'a, Ctx>
where
    Ctx: 'static + Clone + Send,
{
    orb_id: &'a str,
    service_name: &'a str,
    session: zenoh::Session,
    ctx: Ctx,
    subscribers: Vec<(&'static str, Callback<Ctx, Sample>, SubscriberConfig)>,
    queryables: Vec<(&'static str, Callback<Ctx, Query>)>,
}

enum SubscriberConfig {
    Regular,
    /// Subscriber should query for cached messages
    QueryWithTimeout(Duration),
}

impl<'a, Ctx> Receiver<'a, Ctx>
where
    Ctx: 'static + Clone + Send,
{
    pub(crate) fn new(
        orb_id: &'a OrbId,
        service_name: &'a str,
        session: zenoh::Session,
        ctx: Ctx,
    ) -> Self {
        Self {
            orb_id: orb_id.as_str(),
            service_name,
            session,
            ctx,
            subscribers: Vec::new(),
            queryables: Vec::new(),
        }
    }

    pub fn subscriber<H, Fut>(mut self, keyexpr: &'static str, callback: H) -> Self
    where
        H: Fn(Ctx, Sample) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<()>> + 'static + Send,
    {
        self.subscribers.push((
            keyexpr,
            Box::new(move |ctx, sample| Box::pin(callback(ctx, sample))),
            SubscriberConfig::Regular,
        ));
        self
    }

    /// A subscriber that upon subscription will first query for any previously cached
    /// responses for subscribed keyexpr.
    /// ## Note: the longer the timeout, the longer there is a chance of a duplicate when listening to messages stored in the router.
    pub fn querying_subscriber<H, Fut>(
        mut self,
        keyexpr: &'static str,
        query_timeout: Duration,
        callback: H,
    ) -> Self
    where
        H: Fn(Ctx, Sample) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<()>> + 'static + Send,
    {
        self.subscribers.push((
            keyexpr,
            Box::new(move |ctx, sample| Box::pin(callback(ctx, sample))),
            SubscriberConfig::QueryWithTimeout(query_timeout),
        ));
        self
    }

    pub fn queryable<C, Fut>(mut self, keyexpr: &'static str, callback: C) -> Self
    where
        C: Fn(Ctx, Query) -> Fut + 'static + Send + Sync,
        Fut: Future<Output = Result<()>> + 'static + Send,
    {
        self.queryables.push((
            keyexpr,
            Box::new(move |ctx, sample| Box::pin(callback(ctx, sample))),
        ));
        self
    }

    pub async fn run(self) -> Result<Vec<JoinHandle<()>>> {
        let mut tasks = Vec::new();

        for (keyexpr, callback, cfg) in self.subscribers {
            let keyexpr = format!("{}/{keyexpr}", self.orb_id);

            let subscriber = self
                .session
                .declare_subscriber(keyexpr.clone())
                .await
                .map_err(|e| eyre!("{e}"))?;

            let queryfut = match cfg {
                SubscriberConfig::Regular => None,
                SubscriberConfig::QueryWithTimeout(timeout) => {
                    let query = self
                        .session
                        .get(keyexpr.clone())
                        .await
                        .map_err(|e| eyre!("{e}"))?;

                    Some(async move {
                        let deadline = Instant::now() + timeout;
                        let mut samples = Vec::new();

                        while let Ok(Ok(reply)) =
                            time::timeout_at(deadline, query.recv_async()).await
                        {
                            if let Ok(sample) = reply.into_result() {
                                samples.push(sample);
                            }
                        }

                        samples
                    })
                }
            };

            let ctx = self.ctx.clone();
            let handle = task::spawn(async move {
                if let Some(queryfut) = queryfut {
                    for sample in queryfut.await {
                        if let Err(e) = callback(ctx.clone(), sample).await {
                            tracing::error!(
                                "Subscriber for keyexpr '{}' failed with {e}",
                                subscriber.key_expr()
                            );
                        }
                    }
                }

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

                    if let Err(e) = callback(ctx.clone(), sample).await {
                        tracing::error!(
                            "Subscriber for keyexpr '{}' failed with {e}",
                            subscriber.key_expr()
                        );
                    }
                }
            });

            tasks.push(handle);
        }

        for (keyexpr, callback) in self.queryables {
            let queryable = self
                .session
                .declare_queryable(format!(
                    "{}/{}/{keyexpr}",
                    self.orb_id, self.service_name
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

                    if let Err(e) = callback(ctx.clone(), query).await {
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
