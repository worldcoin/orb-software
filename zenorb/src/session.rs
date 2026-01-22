use std::error::Error;

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
    key_expr::KeyExpr,
    query::Selector,
    session::{SessionGetBuilder, SessionPutBuilder},
};

pub struct Session {
    session: zenoh::Session,
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
            orb_id,
            name: name.into(),
        })
    }

    pub fn sender(&self) -> sender::Builder<'_> {
        sender::Builder::new(self.session.clone(), &self.name, &self.orb_id)
    }

    pub fn receiver<Ctx>(&self, ctx: Ctx) -> receiver::Receiver<'_, Ctx>
    where
        Ctx: 'static + Clone + Send,
    {
        receiver::Receiver::new(&self.orb_id, &self.name, self.session.clone(), ctx)
    }

    pub fn put<'a, 'b: 'a, TryIntoKeyExpr, IntoZBytes>(
        &'a self,
        key_expr: TryIntoKeyExpr,
        payload: IntoZBytes,
    ) -> SessionPutBuilder<'a, 'b>
    where
        TryIntoKeyExpr: TryInto<KeyExpr<'b>>,
        <TryIntoKeyExpr as TryInto<KeyExpr<'b>>>::Error:
            Into<Box<dyn Error + Send + Sync + 'static>>,
        IntoZBytes: Into<ZBytes>,
    {
        self.session.put(key_expr, payload)
    }

    pub fn get<'a, 'b: 'a, TryIntoSelector, IntoZBytes>(
        &'a self,
        key_expr: TryIntoSelector,
    ) -> SessionGetBuilder<'a, 'b, DefaultHandler>
    where
        TryIntoSelector: TryInto<Selector<'b>>,
        <TryIntoSelector as TryInto<Selector<'b>>>::Error:
            Into<Box<dyn Error + Send + Sync + 'static>>,
    {
        self.session.get(key_expr)
    }
}
