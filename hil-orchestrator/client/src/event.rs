use crossterm::event::EventStream;
use futures::StreamExt;
use orb_hil_types::{ResultRecord, RunnerStatus};
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};
use tracing::warn;

pub enum Event {
    Key(crossterm::event::KeyEvent),
    Tick,
    RunnersUpdated(Vec<RunnerStatus>),
    ResultsUpdated(Vec<ResultRecord>),
    ApiError(String),
}

pub async fn keyboard_task(tx: mpsc::Sender<Event>) {
    let mut stream = EventStream::new();
    loop {
        match stream.next().await {
            Some(Ok(crossterm::event::Event::Key(key))) => {
                if tx.send(Event::Key(key)).await.is_err() {
                    break;
                }
            }
            Some(Err(e)) => {
                warn!("keyboard event error: {e}");
            }
            None => break,
            _ => {}
        }
    }
}

pub async fn poller_task(
    tx: mpsc::Sender<Event>,
    client: reqwest::Client,
    orchestrator_url: String,
) {
    let mut ticker = interval(std::time::Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let url = format!("{orchestrator_url}/runners");
        match client.get(&url).send().await {
            Ok(resp) => match resp.json::<Vec<RunnerStatus>>().await {
                Ok(runners) => {
                    if tx.send(Event::RunnersUpdated(runners)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    warn!("failed to deserialize runners: {e}");
                    let _ = tx.send(Event::ApiError(e.to_string())).await;
                }
            },
            Err(e) => {
                warn!("failed to poll runners: {e}");
                let _ = tx.send(Event::ApiError(e.to_string())).await;
            }
        }
    }
}
