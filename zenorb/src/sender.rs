use color_eyre::eyre::{eyre, ContextCompat};
use color_eyre::Result;
use orb_info::OrbId;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use zenoh::pubsub::{Publisher, PublisherBuilder};
use zenoh::query::{Querier, QuerierBuilder, ReplyError};
use zenoh::sample::Sample;

#[derive(Clone)]
pub struct Sender {
    registry: Arc<Registry>,
}

struct Registry {
    publishers: HashMap<&'static str, Publisher<'static>>,
    queriers: HashMap<&'static str, Querier<'static>>,
}

impl Sender {
    pub fn publisher(&self, keyexpr: &str) -> Result<&Publisher<'_>> {
        self.registry
            .publishers
            .get(keyexpr)
            .wrap_err_with(|| format!("no declared publisher for keyexpr {keyexpr}"))
    }

    pub fn querier(&self, keyexpr: &str) -> Result<&Querier<'_>> {
        self.registry
            .queriers
            .get(keyexpr)
            .wrap_err_with(|| format!("no declared querier for keyexpr {keyexpr}"))
    }

    /// Sends a JSON command through a declared querier and waits for a single reply.
    ///
    /// The payload is serialized with `serde_json` before being sent.
    ///
    /// This is the sender-side counterpart to `query.json()` and is typically
    /// decoded with `ReplyExt::json()` after awaiting the command.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let reply = sender.command(
    ///     "blue/status",
    ///     &StatusRequest {
    ///         id: 7,
    ///         label: "banana".to_string(),
    ///     },
    /// ).await?;
    ///
    /// let reply: Result<StatusResponse, StatusError> = reply.json()?;
    /// ```
    pub async fn command(
        &self,
        keyexpr: &str,
        payload: impl Serialize,
    ) -> Result<Result<Sample, ReplyError>> {
        let payload = serde_json::to_vec(&payload).map_err(|e| eyre!("{e}"))?;

        let reply = self
            .querier(keyexpr)?
            .get()
            .payload(payload)
            .await
            .map_err(|e| eyre!("{e}"))?
            .recv_async()
            .await
            .map_err(|e| eyre!("{e}"))?;

        Ok(reply.into_result())
    }

    /// Sends a raw command payload through a declared querier and waits for a single reply.
    ///
    /// This does not serialize the payload. It is intended for raw command formats,
    /// such as the space-delimited payload consumed by `query.args()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let reply = sender.command_raw("blue/tuple", "one two").await?;
    /// let reply: Result<(String, String), String> = reply.json()?;
    /// ```
    pub async fn command_raw(
        &self,
        keyexpr: &str,
        payload: &str,
    ) -> Result<Result<Sample, ReplyError>> {
        let reply = self
            .querier(keyexpr)?
            .get()
            .payload(payload)
            .await
            .map_err(|e| eyre!("{e}"))?
            .recv_async()
            .await
            .map_err(|e| eyre!("{e}"))?;

        Ok(reply.into_result())
    }
}

type PublisherBuilderFn =
    for<'a> fn(PublisherBuilder<'a, 'static>) -> PublisherBuilder<'a, 'static>;
type QuerierBuilderFn =
    for<'a> fn(QuerierBuilder<'a, 'static>) -> QuerierBuilder<'a, 'static>;

pub struct Builder<'a> {
    session: &'a zenoh::Session,
    orb_id: &'a str,
    service_name: &'a str,
    publishers: Vec<(&'static str, PublisherBuilderFn)>,
    queriers: Vec<(&'static str, QuerierBuilderFn)>,
}

impl<'a> Builder<'a> {
    pub(crate) fn new(
        session: &'a zenoh::Session,
        service_name: &'a str,
        orb_id: &'a OrbId,
    ) -> Builder<'a> {
        Builder {
            session,
            orb_id: orb_id.as_str(),
            service_name,
            publishers: Vec::new(),
            queriers: Vec::new(),
        }
    }

    /// <env>/<orb-id>/<service-name>/<keyexpr>
    pub fn publisher(self, keyexpr: &'static str) -> Self {
        self.publisher_with(keyexpr, |p| p)
    }

    pub fn publisher_with(
        mut self,
        keyexpr: &'static str,
        f: PublisherBuilderFn,
    ) -> Self {
        self.publishers.push((keyexpr, f));
        self
    }

    pub fn querier(self, keyexpr: &'static str) -> Self {
        self.querier_with(keyexpr, |p| p)
    }

    pub fn querier_with(mut self, keyexpr: &'static str, f: QuerierBuilderFn) -> Self {
        self.queriers.push((keyexpr, f));
        self
    }

    pub async fn build(self) -> Result<Sender> {
        let mut publishers = HashMap::new();
        let mut queriers = HashMap::new();

        for (keyexpr, builder) in self.publishers {
            let full_keyexpr =
                format!("{}/{}/{keyexpr}", self.orb_id, self.service_name);

            let publisher = self.session.declare_publisher(full_keyexpr);
            let publisher = builder(publisher).await.map_err(|e| eyre!("{e}"))?;

            publishers.insert(keyexpr, publisher);
        }

        for (keyexpr, builder) in self.queriers {
            let full_keyexpr = format!("{}/{keyexpr}", self.orb_id);

            let querier = self.session.declare_querier(full_keyexpr);
            let querier = builder(querier).await.map_err(|e| eyre!("{e}"))?;

            queriers.insert(keyexpr, querier);
        }

        Ok(Sender {
            registry: Arc::new(Registry {
                publishers,
                queriers,
            }),
        })
    }
}
