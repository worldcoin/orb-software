use async_tempfile::TempFile;
use fixture::Fixture;
use iroh_blobs::{store::fs::FsStore, Hash};
use reqwest::{Client, StatusCode};
use serde_json::json;
use std::str::FromStr;
use tokio::fs;

mod fixture;

#[tokio::test]
async fn it_adds_a_file_to_the_store() {
    color_eyre::install().unwrap();
    tracing_subscriber::fmt::init();

    // Arrange
    let mut fx = Fixture::builder().well_known_nodes(vec![]).build().await;
    let client = Client::new();
    let store_path = fx.blob_store.dir_path().to_str().unwrap().to_owned();

    let file = TempFile::new().await.unwrap();
    let file_path = file.file_path().to_str().unwrap();
    let expected = "wubalubadubdub";
    fs::write(file_path, expected).await.unwrap();

    // Act
    let res = client
        .post(format!("http://{}/blob", fx.addr))
        .json(&json!({ "path": file_path }))
        .send()
        .await
        .unwrap();

    let status = res.status();
    let hash_str = res.text().await.unwrap();

    // Assert
    fx.stop_server().await;
    let store = FsStore::load(store_path).await.unwrap();
    let hash = Hash::from_str(&hash_str).unwrap();
    let actual = store.blobs().get_bytes(hash).await.unwrap();

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(actual.to_vec(), expected.as_bytes());
}
