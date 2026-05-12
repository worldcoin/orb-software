use std::sync::Arc;

use crate::{
    receiver,
    sender::{self},
};
use bon::bon;
use color_eyre::{eyre::eyre, Result};
use orb_info::OrbId;
use serde::Serialize;
use zenoh::{
    bytes::ZBytes,
    handlers::DefaultHandler,
    pubsub::SubscriberBuilder,
    query::{QueryableBuilder, ReplyError},
    sample::Sample,
    session::{SessionGetBuilder, SessionPutBuilder},
};

#[derive(Clone, Debug)]
pub struct Zenorb {
    session: zenoh::Session,
    meta: Arc<Metadata>,
}

#[derive(Debug)]
struct Metadata {
    orb_id: OrbId,
    name: String,
}

#[bon]
impl Zenorb {
    #[builder(start_fn=from_cfg, finish_fn=with_name)]
    pub async fn new(
        #[builder(start_fn)] cfg: zenoh::Config,
        #[builder(finish_fn)] name: impl Into<String>,
        orb_id: OrbId,
    ) -> Result<Self> {
        let session = zenoh::open(cfg).await.map_err(|e| eyre!("{e}"))?;

        Ok(Self {
            session,
            meta: Arc::new(Metadata {
                orb_id,
                name: name.into(),
            }),
        })
    }

    /// Creates a new `zenorb::Sender`, a registry of declared publishers
    /// and queriers.
    pub fn sender(&self) -> sender::Builder<'_> {
        sender::Builder::new(&self.session, &self.meta.name, &self.meta.orb_id)
    }

    /// Creates a new `zenoh::Receiver`, allowing the registering of subscribers
    /// and queryables that share a context (`Ctx`)
    pub fn receiver<Ctx>(&self, ctx: Ctx) -> receiver::Receiver<'_, Ctx>
    where
        Ctx: 'static + Clone + Send,
    {
        receiver::Receiver::new(&self.meta.orb_id, &self.meta.name, &self.session, ctx)
    }

    /// This wrapper prefixes the key expression with `"{orb_id}/{name}/"`.
    /// See [`zenoh::Session::put`] for the full semantics.
    pub fn put<'a>(
        &'a self,
        keyexpr: &str,
        payload: impl Into<ZBytes>,
    ) -> SessionPutBuilder<'a, 'a> {
        self.session.put(
            format!("{}/{}/{keyexpr}", self.meta.orb_id, self.meta.name),
            payload,
        )
    }

    /// This wrapper prefixes the key expression with `"{orb_id}/"`.
    /// See [`zenoh::Session::get`] for full documentation.
    pub fn get<'a>(
        &'a self,
        keyexpr: &str,
    ) -> SessionGetBuilder<'a, 'a, DefaultHandler> {
        self.session.get(format!("{}/{keyexpr}", self.meta.orb_id))
    }

    /// Sends a JSON command to `"{orb_id}/{keyexpr}"` and waits for a single reply.
    ///
    /// The payload is serialized with `serde_json` before being sent. The return
    /// value preserves Zenoh's distinction between transport failures and reply
    /// errors:
    ///
    /// - outer `Result`: query transport or session failure
    /// - inner `Result`: successful reply vs. `reply_err(...)`
    ///
    /// This is best paired with `query.json()` on the queryable side and
    /// `ReplyExt::json()` on the caller side.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let reply = zenorb.command(
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
            .get(keyexpr)
            .payload(payload)
            .await
            .map_err(|e| eyre!("{e}"))?
            .recv_async()
            .await
            .map_err(|e| eyre!("{e}"))?;

        Ok(reply.into_result())
    }

    /// Sends a raw command payload to `"{orb_id}/{keyexpr}"` and waits for a single reply.
    ///
    /// Unlike [`command`](Zenorb::command), this method does not JSON-serialize the
    /// payload first. It is intended for raw text protocols such as the
    /// space-delimited format consumed by `query.args()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let reply = zenorb.command_raw("blue/tuple", "one two").await?;
    /// let reply: Result<(String, String), String> = reply.json()?;
    /// ```
    pub async fn command_raw(
        &self,
        keyexpr: &str,
        payload: &str,
    ) -> Result<Result<Sample, ReplyError>> {
        let reply = self
            .get(keyexpr)
            .payload(payload)
            .await
            .map_err(|e| eyre!("{e}"))?
            .recv_async()
            .await
            .map_err(|e| eyre!("{e}"))?;

        Ok(reply.into_result())
    }

    /// This wrapper prefixes the key expression with `"{orb_id}/"`.
    /// See [`zenoh::Session::declare_subscriber`] for full documentation.
    pub fn declare_subscriber<'a>(
        &'a self,
        keyexpr: &str,
    ) -> SubscriberBuilder<'a, 'a, DefaultHandler> {
        self.session
            .declare_subscriber(format!("{}/{keyexpr}", self.meta.orb_id))
    }

    /// This wrapper prefixes the key expression with `"{orb_id}/"`.
    ///
    /// This exposes the raw Zenoh queryable API when you want to receive and
    /// handle queries directly instead of going through `Receiver::queryable(...)`.
    /// See [`zenoh::Session::declare_queryable`] for full documentation.
    pub fn declare_queryable<'a>(
        &'a self,
        keyexpr: &str,
    ) -> QueryableBuilder<'a, 'a, DefaultHandler> {
        self.session.declare_queryable(format!(
            "{}/{}/{keyexpr}",
            self.meta.orb_id, self.meta.name
        ))
    }

    /// Exposes the underlying [`zenoh::Session`]
    pub fn session(&self) -> &zenoh::Session {
        &self.session
    }

    pub fn orb_id(&self) -> &OrbId {
        &self.meta.orb_id
    }
}
