use crate::collectors::oes::Event;
use color_eyre::{eyre::eyre, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

// hacky, tech debt, will be removed soon
#[derive(Clone, Default)]
pub struct OesEventCache(Arc<Mutex<HashMap<String, Event>>>);

impl OesEventCache {
    pub fn insert(&self, evt: Event) -> Result<()> {
        let mut cache = self.0.lock().map_err(|_| eyre!("cache lock poison"))?;
        cache.insert(evt.name.clone(), evt);

        Ok(())
    }

    pub fn values(&self) -> Result<Vec<Event>> {
        let values = self
            .0
            .lock()
            .map_err(|_| eyre!("cache lock poison"))?
            .values()
            .cloned()
            .collect();

        Ok(values)
    }
}
