use bytes::Bytes;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::{Stream, StreamExt, TryStreamExt as _};
use sha2::{Digest, Sha256};
use std::{io::Result as IoResult, pin::pin, time::Duration};
use tokio::{
    fs::File,
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite,
        AsyncWriteExt, BufWriter,
    },
    sync::mpsc,
};
use tokio_util::io::SinkWriter;

use crate::diff_plan::{DiffPlan, Operation};

const CHANNEL_CAPACITY: usize = 16;

#[derive(Debug)]
pub struct DiffPlanOutputs {}

#[derive(Debug)]
pub struct FileSummary {
    /// Hash of output file
    pub hash: Vec<u8>,
    /// Size in bytes
    pub size: u64,
}

pub async fn execute_plan(plan: &DiffPlan) -> Result<DiffPlanOutputs> {
    let mut summary_tasks = tokio::task::JoinSet::new();
    //let mut copy_tasks = tokio::task::JoinSet::new();
    let mut copy_op_tasks = tokio::task::JoinSet::new();
    let mut bidiff_op_tasks = tokio::task::JoinSet::new();
    let multi_bar = indicatif::MultiProgress::new();
    for op in plan.ops.iter() {
        let (chunk_tx, chunk_rx) = mpsc::channel(CHANNEL_CAPACITY);
        summary_tasks.spawn_blocking(|| summary_task_entry(chunk_rx));
        let pb: indicatif::ProgressBar = match op {
            Operation::Bidiff {
                id,
                old_path,
                new_path,
                out_path,
            } => {
                let mdata = bidiff_op_tasks.spawn(tokio::time::timeout(
                    Duration::from_secs(5),
                    std::future::pending::<()>(),
                ));

                indicatif::ProgressBar::no_length()
                    .with_style(crate::progress_bar_style())
            }
            Operation::Copy {
                id,
                from_path,
                to_path,
            } => {
                let mdata =
                    tokio::fs::metadata(&from_path).await.wrap_err_with(|| {
                        format!("failed to read metadata of file `{from_path:?}`")
                    })?;
                copy_op_tasks.spawn(tokio::time::timeout(
                    Duration::from_secs(5),
                    std::future::pending::<()>(),
                ));
                indicatif::ProgressBar::new(mdata.len())
                    .with_style(crate::progress_bar_style())
            }
        };
        multi_bar.add(pb);
    }

    tokio::time::sleep(Duration::from_secs(10)).await;

    todo!("join on the tasks")
}

fn summary_task_entry(mut chunk_rx: mpsc::Receiver<Bytes>) -> Result<FileSummary> {
    let mut hasher = Sha256::new();
    let mut size = 0;
    while let Some(chunk) = chunk_rx.blocking_recv() {
        hasher.update(&chunk);
        size += u64::try_from(chunk.len()).expect("overflow");
    }

    let hash = hasher.finalize();
    let hash = Vec::from(hash.as_slice());

    Ok(FileSummary { hash, size })
}

async fn copy_task_entry(
    from: impl AsyncRead,
    to: impl AsyncWrite,
    summary_tx: mpsc::Sender<Bytes>,
) -> Result<()> {
    let from = pin!(from);
    let mut to = pin!(to);
    let mut from = tokio_util::io::ReaderStream::new(from);
    while let Some(chunk) = from.try_next().await? {
        if let Err(_) = summary_tx.send(Bytes::clone(&chunk)).await {
            // Summarizer closed, our task should just terminate cleanly to avoid
            // confusing errors.
            return Ok(());
        }
        to.write_all(chunk.as_ref()).await?;
    }

    Ok(())
}
