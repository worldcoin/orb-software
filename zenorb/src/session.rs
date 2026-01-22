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

    pub fn put<'a>(
        &'a self,
        keyexpr: &str,
        payload: impl Into<ZBytes>,
    ) -> SessionPutBuilder<'a, 'a> {
        self.session
            .put(format!("{}/{}/{keyexpr}", self.orb_id, self.name), payload)
    }

    pub fn get<'a>(
        &'a self,
        keyexpr: &str,
    ) -> SessionGetBuilder<'a, 'a, DefaultHandler> {
        self.session.get(format!("{}/{keyexpr}", self.orb_id))
    }
}
