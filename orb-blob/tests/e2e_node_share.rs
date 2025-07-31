use std::time::Duration;

use async_tempfile::TempFile;
use fixture::Fixture;
use iroh::SecretKey;
use reqwest::{Client, StatusCode};
use serde_json::json;
use tokio::fs;

mod fixture;

<<<<<<< HEAD
#[tokio::test]
=======
#[allow(dead_code)]
>>>>>>> c47713fcbff89c1d7c61c6da7670a6c3e5dbf7dc
async fn it_shares_files_across_nodes() {
    // Arrange
    let upload_fx_key =
        SecretKey::from_bytes("x".repeat(32).as_bytes().try_into().unwrap());

    let download_fx_key =
        SecretKey::from_bytes("a".repeat(32).as_bytes().try_into().unwrap());

    let well_known_nodes = vec![upload_fx_key.public(), download_fx_key.public()];

    let upload_fx = Fixture::builder()
        .secret_key(upload_fx_key)
        .well_known_nodes(well_known_nodes.clone())
        .build()
        .await;

    let download_fx = Fixture::builder()
        .secret_key(download_fx_key)
        .min_peer_req(1)
        .well_known_nodes(well_known_nodes)
<<<<<<< HEAD
        .peer_listen_timeout(Duration::from_secs(10))
=======
        .peer_listen_timeout(Duration::from_secs(5))
>>>>>>> c47713fcbff89c1d7c61c6da7670a6c3e5dbf7dc
        .build()
        .await;

    let upload_file = TempFile::new().await.unwrap();
    let upload_file_path = upload_file.file_path().to_str().unwrap();
    let expected = "wubalubadubdub";
    fs::write(upload_file_path, expected).await.unwrap();

    let download_file = TempFile::new().await.unwrap();
    let download_file_path = download_file.file_path().to_str().unwrap();

    let client = Client::new();

    // Upload
    let res = client
        .post(format!("http://{}/blob", upload_fx.addr))
        .json(&json!({ "path": upload_file_path }))
        .send()
        .await
        .unwrap();

    let status = res.status();
    let uploaded_hash = res.text().await.unwrap();
    assert_eq!(status, StatusCode::CREATED);

    // Download
    let res = client
        .post(format!("http://{}/download", download_fx.addr))
        .json(&json!({ "hash": uploaded_hash, "download_path": download_file_path }))
        .send()
        .await
        .unwrap();

    let actual = fs::read(download_file_path).await.unwrap();
    let actual = String::from_utf8(actual).unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);
    assert_eq!(actual, expected);
}
