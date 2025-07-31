use async_tempfile::TempFile;
use fixture::Fixture;
use iroh_blobs::{store::fs::FsStore, Hash};
use reqwest::{Client, StatusCode};
use serde_json::json;
use std::str::FromStr;
use tokio::fs;
mod fixture;

#[allow(dead_code)]
async fn it_deletes_a_file_from_store() {
    let mut fx = Fixture::builder().build().await;
    let client = Client::new();
    let store_path = fx.blob_store.dir_path().to_owned();

    let file = TempFile::new().await.unwrap();

    fs::write(file.file_path(), b"delete me bro I beg you")
        .await
        .unwrap();

    let hash = client
        .post(format!("http://{}/blob", fx.addr))
        .json(&json!({"path": file.file_path()}))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let res = client
        .delete(format!("http://{}/blob/{}", fx.addr, hash))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    fx.stop_server().await;

    let store = FsStore::load(store_path).await.unwrap();

    let f = store
        .blobs()
        .get_bytes(Hash::from_str(&hash).unwrap())
        .await
        .unwrap();

    println!("Found something: {:?}", f);
    assert!(!store
        .blobs()
        .has(Hash::from_str(&hash).unwrap())
        .await
        .unwrap(),)
}
