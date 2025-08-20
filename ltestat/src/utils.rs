use std::sync::{
    Arc, LockResult, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
};

use color_eyre::{eyre::eyre, Result};
use tokio::process::Command;

pub async fn run_cmd(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd).args(args).output().await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        let args = args.join(" ");
        Err(eyre!("Failed to run {cmd} {args}. Error {err}"))
    }
}
pub fn retrieve_value(output: &str, key: &str) -> Result<String> {
    output
        .lines()
        .find(|l| l.starts_with(key))
        .ok_or_else(|| eyre!("Key {key} not found"))?
        .split_once(':')
        .ok_or_else(|| eyre!("Malformed line for key {key}"))
        .map(|(_, v)| v.trim().to_string())
}

pub struct State<T> {
    state: Arc<RwLock<T>>,
}

impl<T> State<T> {
    pub fn new(state: T) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
        }
    }
    pub fn read<F, K>(&self, f: F) -> Result<K, PoisonError<RwLockReadGuard<'_, T>>>
    where
        F: FnOnce(&T) -> K,
    {
        let value = self.state.read()?;
        Ok(f(&value))
    }

    pub fn write<F>(&self, f: F) -> Result<(), PoisonError<RwLockWriteGuard<'_, T>>>
    where
        F: FnOnce(&mut T),
    {
        let mut value = self.state.write()?;
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
