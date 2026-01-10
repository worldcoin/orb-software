use crate::{
    receiver,
    sender::{self},
};
use bon::bon;
use color_eyre::{eyre::eyre, Result};
use orb_info::{orb_os_release::OrbRelease, OrbId};

pub struct Session {
    session: zenoh::Session,
    env: OrbRelease,
    orb_id: OrbId,
    service_name: String,
}

#[bon]
impl Session {
    #[builder(start_fn=from_cfg, finish_fn=for_service)]
    pub async fn new(
        #[builder(start_fn)] cfg: zenoh::Config,
        #[builder(finish_fn)] service_name: &str,
        env: OrbRelease,
        orb_id: OrbId,
    ) -> Result<Self> {
        let session = zenoh::open(cfg).await.map_err(|e| eyre!("{e}"))?;

        Ok(Self {
            session,
            env,
            orb_id,
            service_name: service_name.into(),
        })
    }

    pub fn sender(&self) -> sender::Builder<'_> {
        sender::Builder::new(
            self.session.clone(),
            &self.env,
            &self.service_name,
            &self.orb_id,
        )
    }

    pub fn receiver<Ctx>(&self, ctx: Ctx) -> receiver::Receiver<'_, Ctx>
    where
        Ctx: 'static + Clone + Send,
    {
        receiver::Receiver::new(
            &self.env,
            &self.orb_id,
            &self.service_name,
            self.session.clone(),
            ctx,
        )
    }
}
