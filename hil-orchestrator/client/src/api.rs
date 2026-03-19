use orb_hil_types::{ResultRecord, RunnerStatus};

use crate::app::ResultsFilter;

pub async fn fetch_runners(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<RunnerStatus>, String> {
    let endpoint = format!("{url}/runners");
    let resp = client
        .get(&endpoint)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    resp.json::<Vec<RunnerStatus>>()
        .await
        .map_err(|e| e.to_string())
}

pub async fn lock_runner(
    client: &reqwest::Client,
    url: &str,
    id: &str,
) -> Result<(), String> {
    let endpoint = format!("{url}/runners/{id}/lock");
    let resp = client
        .post(&endpoint)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        return Ok(());
    }

    // Extract error message from JSON body on non-2xx
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let msg = body
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("lock failed")
        .to_string();

    Err(msg)
}

pub async fn unlock_runner(
    client: &reqwest::Client,
    url: &str,
    id: &str,
) -> Result<(), String> {
    let endpoint = format!("{url}/runners/{id}/unlock");
    let resp = client
        .post(&endpoint)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        return Ok(());
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let msg = body
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("unlock failed")
        .to_string();

    Err(msg)
}

pub async fn fetch_results(
    _client: &reqwest::Client,
    _url: &str,
    _filter: &ResultsFilter,
    _runner_id: Option<&str>,
) -> Result<Vec<ResultRecord>, String> {
    todo!()
}
