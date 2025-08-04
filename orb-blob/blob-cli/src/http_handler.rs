use color_eyre::eyre::Result;
use reqwest::Client;
use serde_json::json;

pub async fn upload(path: &str, client: &Client, base_url: &str) -> Result<()> {
    let body = json!({ "path": path });

    let res = client
        .post(format!("{}/blob", base_url))
        .json(&body)
        .send()
        .await?;

    let status = res.status();
    let hash = res.text().await?;
    println!("Status: {status}\nUploaded Hash: {hash}");
    Ok(())
}

pub async fn download(
    hash: &str,
    dest: &str,
    client: &Client,
    base_url: &str,
) -> Result<()> {
    let body = json!({ "hash": hash, "download_path": dest });

    let res = client
        .post(format!("{}/download", base_url))
        .json(&body)
        .send()
        .await?;

    println!("Status: {}", res.status());
    Ok(())
}

pub async fn info(client: &Client, base_url: &str) -> Result<()> {
    let res = client.get(format!("{}/info", base_url)).send().await?;
    let text = res.text().await?;

    println!("--- Node Info ---\n{text}");
    Ok(())
}
