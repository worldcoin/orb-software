use color_eyre::{eyre::eyre, Result};
use std::{
    sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Duration,
};
use tokio::{
    process::Command,
    task::JoinHandle,
    time::{self, Instant},
};

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

pub async fn retry_for<F, K>(timeout: Duration, backoff: Duration, f: F) -> Result<K>
where
    F: AsyncFn() -> Result<K>,
{
    let start = Instant::now();

    loop {
        match f().await {
            Err(e) => {
                if start.elapsed() >= timeout {
                    return Err(e);
                }

                time::sleep(backoff).await;
            }

            Ok(m) => return Ok(m),
        }
    }
}
