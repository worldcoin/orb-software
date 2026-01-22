use std::sync::Arc;

use crate::{
    receiver,
    sender::{self},
};
use bon::bon;
use color_eyre::{eyre::eyre, Result};
use orb_info::OrbId;
use zenoh::{
    bytes::ZBytes,
    handlers::DefaultHandler,
    session::{SessionGetBuilder, SessionPutBuilder},
    time::Timestamp,
};

#[derive(Clone, Debug)]
pub struct Session {
    session: zenoh::Session,
    meta: Arc<Metadata>,
}

#[derive(Debug)]
struct Metadata {
    orb_id: OrbId,
    name: String,
}

#[bon]
impl Session {
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

    /// Wrapper around [`zenoh::Session::new_timestamp`].
    pub fn new_timestamp(&self) -> Timestamp {
        self.session.new_timestamp()
    }
}
