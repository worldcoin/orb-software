use crate::{
    receiver,
    sender::{self},
};
use bon::bon;
use color_eyre::{eyre::eyre, Result};
use orb_info::OrbId;

pub struct Session {
    session: zenoh::Session,
    orb_id: OrbId,
    service_name: String,
}

#[bon]
impl Session {
    #[builder(start_fn=from_cfg, finish_fn=with_name)]
    pub async fn new(
        #[builder(start_fn)] cfg: zenoh::Config,
        #[builder(finish_fn)] service_name: &str,
        orb_id: OrbId,
    ) -> Result<Self> {
        let session = zenoh::open(cfg).await.map_err(|e| eyre!("{e}"))?;

        Ok(Self {
            session,
            orb_id,
            service_name: service_name.into(),
        })
    }

    pub fn sender(&self) -> sender::Builder<'_> {
        sender::Builder::new(self.session.clone(), &self.service_name, &self.orb_id)
    }

    pub fn receiver<Ctx>(&self, ctx: Ctx) -> receiver::Receiver<'_, Ctx>
    where
        Ctx: 'static + Clone + Send,
    {
        receiver::Receiver::new(
            &self.orb_id,
            &self.service_name,
            self.session.clone(),
            ctx,
        )
    }
}
