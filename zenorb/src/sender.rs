use color_eyre::eyre::{eyre, ContextCompat};
use color_eyre::Result;
use orb_info::orb_os_release::OrbRelease;
use orb_info::OrbId;
use std::collections::HashMap;
use std::sync::Arc;
use zenoh::pubsub::{Publisher, PublisherBuilder};
use zenoh::query::{Querier, QuerierBuilder};

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
}

type PublisherBuilderFn =
    for<'a> fn(PublisherBuilder<'a, 'static>) -> PublisherBuilder<'a, 'static>;
type QuerierBuilderFn =
    for<'a> fn(QuerierBuilder<'a, 'static>) -> QuerierBuilder<'a, 'static>;

pub struct Builder<'a> {
    session: zenoh::Session,
    orb_id: &'a str,
    service_name: &'a str,
    env: &'a str,
    publishers: Vec<(&'static str, PublisherBuilderFn)>,
    queriers: Vec<(&'static str, QuerierBuilderFn)>,
}

impl<'a> Builder<'a> {
    pub(crate) fn new(
        session: zenoh::Session,
        env: &'a OrbRelease,
        service_name: &'a str,
        orb_id: &'a OrbId,
    ) -> Builder<'a> {
        Builder {
            session,
            orb_id: orb_id.as_str(),
            service_name,
            env: env.as_str(),
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
            let full_keyexpr = format!(
                "{}/{}/{}/{keyexpr}",
                self.env, self.orb_id, self.service_name
            );

            let publisher = self.session.declare_publisher(full_keyexpr);
            let publisher = builder(publisher).await.map_err(|e| eyre!("{e}"))?;

            publishers.insert(keyexpr, publisher);
        }

        for (keyexpr, builder) in self.queriers {
            let full_keyexpr = format!("{}/{}/{keyexpr}", self.env, self.orb_id);

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
