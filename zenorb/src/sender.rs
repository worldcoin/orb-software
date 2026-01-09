use bon::bon;
use color_eyre::eyre::{eyre, ContextCompat};
use color_eyre::Result;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::Mutex;
use zenoh::bytes::{Encoding, OptionZBytes, ZBytes};
use zenoh::key_expr::KeyExpr;
use zenoh::pubsub::{PublicationBuilder, Publisher, PublisherBuilder, Subscriber};
use zenoh::qos::{CongestionControl, Priority};
use zenoh::query::{
    ConsolidationMode, Querier, QuerierBuilder, QueryConsolidation, Queryable,
};
use zenoh::sample::Locality;
use zenoh::time::Timestamp;

type Map<K, V> = Arc<Mutex<HashMap<K, V>>>;

#[derive(Clone)]
pub struct Sender {
    registry: Arc<Registry>,
    session: zenoh::Session,
}

struct Registry {
    publishers: HashMap<&'static str, Publisher<'static>>,
    queriers: HashMap<&'static str, Querier<'static>>,
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

impl Sender {
    pub fn publisher(&self, keyexpr: &str) -> Result<&Publisher<'_>> {
        self.registry
            .publishers
            .get(keyexpr)
            .wrap_err_with(|| format!("no declared publisher for keyxpr {keyexpr}"))
    }

    pub fn querier(&self, keyexpr: &str) -> Result<&Querier<'_>> {
        self.registry
            .queriers
            .get(keyexpr)
            .wrap_err_with(|| format!("no declared querier for keyxpr {keyexpr}"))
    }
}

impl<'a> Builder<'a> {
    pub(crate) fn new(
        session: zenoh::Session,
        env: &'a str,
        service_name: &'a str,
        orb_id: &'a str,
    ) -> Builder<'a> {
        Builder {
            session,
            orb_id,
            service_name,
            env,
            publishers: Vec::new(),
            queriers: Vec::new(),
        }
    }

    /// <env>/<orb-id>/<service-name>/<topic>
    pub fn publisher(self, topic: &'static str) -> Self {
        self.publisher_with(topic, |p| p)
    }

    pub fn publisher_with(
        mut self,
        topic: &'static str,
        f: PublisherBuilderFn,
    ) -> Self {
        self.publishers.push((topic, f));
        self
    }

    pub fn querier(self, topic: &'static str) -> Self {
        self.querier_with(topic, |p| p)
    }

    pub fn querier_with(mut self, topic: &'static str, f: QuerierBuilderFn) -> Self {
        self.queriers.push((topic, f));
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
            let publisher = builder(publisher).await.unwrap();

            publishers.insert(keyexpr, publisher);
        }

        for (keyexpr, builder) in self.queriers {
            let full_keyexpr = format!("{}/{}/{keyexpr}", self.env, self.orb_id);

            let querier = self.session.declare_querier(full_keyexpr);
            let querier = builder(querier).await.unwrap();

            queriers.insert(keyexpr, querier);
        }

        Ok(Sender {
            registry: Arc::new(Registry {
                publishers,
                queriers,
            }),

            session: self.session,
        })
    }
}
