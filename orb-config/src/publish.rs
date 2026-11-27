use std::sync::Arc;

use eyre::{ensure, eyre, Result};
use serde_json::{Map, Value};
use tokio::sync::watch;
use zenorb::{
    zenoh::{
        bytes::Encoding,
        key_expr::OwnedKeyExpr,
        qos::CongestionControl,
        query::{Query, Queryable},
        Wait,
    },
    Zenorb,
};

pub struct Publisher {
    session: Zenorb,
    latest: watch::Sender<Arc<Map<String, Value>>>,
    published: Map<String, Value>,
    _queryable: Queryable<()>,
}

pub fn new(session: Zenorb) -> Result<Publisher> {
    let (latest, receiver) = watch::channel(Arc::new(Map::new()));
    let queryable = session
        .session()
        .declare_queryable("config/*")
        .callback(move |query| {
            // Release the watch lock before sending replies.
            let sections = receiver.borrow().clone();
            for (section, value) in sections.iter() {
                if let Err(error) = reply_section(&query, section, value) {
                    tracing::warn!(section, %error, "configuration query reply failed");
                }
            }
        })
        .wait()
        .map_err(|error| eyre!("failed to declare configuration queryable: {error}"))?;
    Ok(Publisher {
        session,
        latest,
        published: Map::new(),
        _queryable: queryable,
    })
}

pub fn publish(publisher: &mut Publisher, sections: Arc<Map<String, Value>>) {
    publisher.latest.send_replace(sections.clone());
    publisher
        .published
        .retain(|section, _| sections.contains_key(section));
    for (section, value) in sections.iter() {
        if publisher.published.get(section) == Some(value) {
            continue;
        }
        match publish_section(&publisher.session, section, value) {
            Ok(()) => {
                publisher.published.insert(section.clone(), value.clone());
                tracing::info!(section, "configuration section published");
            }
            Err(error) => {
                tracing::warn!(section, %error, "configuration section publication failed; retrying next poll");
            }
        }
    }
}

fn section_key(section: &str) -> Result<OwnedKeyExpr> {
    ensure!(
        !section.is_empty() && !section.contains('/'),
        "section must be a single key segment"
    );
    let key = OwnedKeyExpr::new(format!("config/{section}"))
        .map_err(|_| eyre!("invalid configuration section key"))?;
    ensure!(
        !key.is_wild(),
        "configuration section key must not contain wildcards"
    );
    Ok(key)
}

fn publish_section(session: &Zenorb, section: &str, value: &Value) -> Result<()> {
    session
        .session()
        .put(section_key(section)?, serde_json::to_vec(value)?)
        .encoding(Encoding::APPLICATION_JSON)
        .congestion_control(CongestionControl::Drop)
        .wait()
        .map_err(|error| eyre!("Zenoh publication failed: {error}"))
}

fn reply_section(query: &Query, section: &str, value: &Value) -> Result<()> {
    let key = section_key(section)?;
    if query.key_expr().intersects(&key) {
        query
            .reply(key, serde_json::to_vec(value)?)
            .encoding(Encoding::APPLICATION_JSON)
            .congestion_control(CongestionControl::Drop)
            .wait()
            .map_err(|error| eyre!("Zenoh reply failed: {error}"))?;
    }
    Ok(())
}
