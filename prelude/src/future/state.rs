use color_eyre::{eyre::eyre, Result};
use std::sync::{Arc, RwLock};

pub struct State<T> {
    state: Arc<RwLock<T>>,
}

impl<T> State<T> {
    pub fn new(state: T) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
        }
    }

    pub fn read<F, K>(&self, f: F) -> Result<K>
    where
        F: FnOnce(&T) -> K,
    {
        let value = self
            .state
            .read()
            .map_err(|_| eyre!("PoisonError when reading from State<_>"))?;

        Ok(f(&value))
    }

    pub fn write<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut T),
    {
        let mut value = self
            .state
            .write()
            .map_err(|_| eyre!("PoisonError when writing to State<_>"))?;

        f(&mut value);
        Ok(())
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}
