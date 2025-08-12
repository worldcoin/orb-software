use std::{ops::Deref, sync::Arc};
use tokio::sync::{Mutex, MutexGuard};

/// Wrapper for `Arc<Mutex<T>>` used to make testing easier.
#[derive(Debug)]
pub struct AsyncBag<T>(Arc<Mutex<T>>);

impl<T> Clone for AsyncBag<T> {
    fn clone(&self) -> Self {
        AsyncBag(self.0.clone())
    }
}

impl<T> Default for AsyncBag<T>
where
    T: Default,
{
    fn default() -> Self {
        AsyncBag(Arc::new(Mutex::new(T::default())))
    }
}

impl<T> AsyncBag<T> {
    pub fn new(t: T) -> Self {
        AsyncBag(Arc::new(Mutex::new(t)))
    }

    pub async fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().await
    }

    pub async fn set(&self, t: T) {
        let mut old_t = self.0.lock().await;
        *old_t = t;
    }

    pub async fn inspect(&self, f: impl Fn(&T)) {
        f(self.0.lock().await.deref());
    }
}

impl<T> AsyncBag<T>
where
    T: Clone,
{
    /// Returns a clone of the inner value.
    pub async fn read(&self) -> T {
        self.0.lock().await.clone()
    }
}

impl<T> AsyncBag<T>
where
    T: Clone + IntoIterator,
{
    pub async fn map_iter<K>(&self, f: impl Fn(T::Item) -> K) -> Vec<K> {
        self.read().await.clone().into_iter().map(f).collect()
    }
}
