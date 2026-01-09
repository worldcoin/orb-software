use crate::{
    receiver,
    sender::{self, Builder},
    Sender,
};
use bon::bon;
use color_eyre::{eyre::eyre, Result};

pub struct Session {
    session: zenoh::Session,
    orb_id: String,
    service_name: String,
    env: String,
}

#[bon]
impl Session {
    #[builder(start_fn=from_cfg, finish_fn=for_service)]
    pub async fn new(
        #[builder(start_fn)] cfg: zenoh::Config,
        #[builder(finish_fn)] service_name: &str,
        env: &str,
        orb_id: &str,
    ) -> Result<Self> {
        let session = zenoh::open(cfg).await.map_err(|e| eyre!("{e}"))?;

        Ok(Self {
            session,
            orb_id: orb_id.into(),
            service_name: service_name.into(),
            env: env.into(),
        })
    }

    pub fn sender(&self) -> sender::Builder {
        sender::Builder::new(
            self.session.clone(),
            &self.env,
            &self.orb_id,
            &self.service_name,
        )
    }

    pub fn receiver<Ctx>(&self, ctx: Ctx) -> receiver::Receiver<'_, Ctx>
    where
        Ctx: 'static + Clone + Send,
    {
        receiver::Receiver::new(&self.env, &self.orb_id, self.session.clone(), ctx)
    }
}
