#[cfg(not(test))]
pub async fn orb_uptime() -> Option<f64> {
    let uptime = tokio::fs::read_to_string("/proc/uptime").await.ok()?;
    let uptime = uptime.split_whitespace().next().unwrap();
    Some(uptime.parse::<f64>().unwrap())
}

#[cfg(test)]
pub async fn orb_uptime() -> Option<f64> {
    Some(100.0)
}
