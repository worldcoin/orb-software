use crate::{modem::Modem, utils::State};
use std::time::Duration;
use tokio::task::{self, JoinHandle};

pub fn start(state: State<Modem>, report_interval: Duration) -> JoinHandle<()> {
    task::spawn(async move {})
}
