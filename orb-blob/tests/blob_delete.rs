use async_tempfile::TempFile;
use fixture::Fixture;
use iroh_blobs::{
    store::fs::{
        options::{BatchOptions, GcConfig, InlineOptions, Options, PathOptions},
        FsStore,
    },
    Hash,
};
use reqwest::{Client, StatusCode};
use serde_json::json;
use std::{str::FromStr, time::Duration};
use tokio::{fs, time};
mod fixture;

#[tokio::test]
async fn it_deletes_a_file_from_store() {
    color_eyre::install().unwrap();
    tracing_subscriber::fmt::init();

    let mut fx = Fixture::builder().well_known_nodes(vec![]).build().await;
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

    let db_path = store_path.join("blobs.db");
    let options = Options {
        path: PathOptions::new(&store_path),
        inline: InlineOptions::default(),
        batch: BatchOptions::default(),
        gc: Some(GcConfig {
            interval: Duration::from_secs(1),
            add_protected: None,
        }),
    };

    let store = FsStore::load_with_opts(db_path, options).await.unwrap();

    // Sleep so that GC has time to run
    time::sleep(Duration::from_secs(2)).await;

    assert!(!store
        .blobs()
        .has(Hash::from_str(&hash).unwrap())
        .await
        .unwrap(),)
}
