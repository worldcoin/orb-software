//! Tests for the helpers in common

pub mod common;

use common::compare_file_to_buf;

use async_tempfile::TempFile;
use color_eyre::{eyre::Context as _, Result};
use tokio::io::{AsyncSeekExt as _, AsyncWriteExt};

async fn create_temp_file_with_contents(contents: &[u8]) -> Result<TempFile> {
    let mut file = TempFile::new()
        .await
        .wrap_err("failed to create temp file")?;
    file.write_all(contents)
        .await
        .wrap_err("failed to write temp file's contents")?;
    file.flush().await?;
    file.sync_all().await?;
    file.rewind().await?;

    Ok(file)
}

#[tokio::test]
async fn test_1kb_file() -> Result<()> {
    let data = vec![42u8; 1024];
    let file = create_temp_file_with_contents(&data).await?;
    compare_file_to_buf(file, &data).await?;
    Ok(())
}

#[tokio::test]
async fn test_1mb_file() -> Result<()> {
    let data = vec![123u8; 1024 * 1024];
    let file = create_temp_file_with_contents(&data).await?;
    compare_file_to_buf(file, &data).await?;
    Ok(())
}

#[tokio::test]
async fn test_empty_file() -> Result<()> {
    let data = vec![];
    let file = create_temp_file_with_contents(&data).await?;
    compare_file_to_buf(file, &data).await?;
    Ok(())
}

#[tokio::test]
async fn test_single_byte_file() -> Result<()> {
    let data = vec![255u8];
    let file = create_temp_file_with_contents(&data).await?;
    compare_file_to_buf(file, &data).await?;
    Ok(())
}

#[tokio::test]
async fn test_odd_length_file() -> Result<()> {
    let data = vec![1u8; 8193]; // One byte more than 8KiB
    let file = create_temp_file_with_contents(&data).await?;
    compare_file_to_buf(file, &data).await?;
    Ok(())
}

#[tokio::test]
async fn test_mismatch_content() -> Result<()> {
    let data = vec![1u8; 100];
    let file = create_temp_file_with_contents(&data).await?;
    let different_data = vec![2u8; 100];
    assert!(compare_file_to_buf(file, &different_data).await.is_err());
    Ok(())
}

#[tokio::test]
async fn test_mismatch_length() -> Result<()> {
    let data = vec![1u8; 100];
    let file = create_temp_file_with_contents(&data).await?;
    let shorter_data = vec![1u8; 50];
    assert!(compare_file_to_buf(file, &shorter_data).await.is_err());
    Ok(())
}
