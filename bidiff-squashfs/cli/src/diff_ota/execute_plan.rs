use std::collections::HashMap;
use std::pin::pin;
use std::{io::Result as IoResult, path::Path};

use bidiff::DiffParams;
use bytes::Bytes;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::Stream;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt as _;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;
use tracing::info;

use super::diff_plan::{ComponentId, DiffPlan, Operation};

const CHANNEL_CAPACITY: usize = 16;

#[derive(Debug)]
pub struct DiffPlanOutputs {
    pub summaries: HashMap<ComponentId, FileSummary>,
}

#[derive(Debug)]
pub struct FileSummary {
    /// Hash of output file
    pub hash: Vec<u8>,
    /// Size in bytes
    pub size: u64,
}

/// The three potential types of tasks involved in executing an operation.
struct OpTasks {
    id: ComponentId,
    copy: JoinHandle<Result<()>>,
    summary: JoinHandle<FileSummary>,
    processing: Option<JoinHandle<Result<()>>>,
}

impl OpTasks {
    async fn try_join(self) -> Result<FileSummary> {
        let copy_fut = async {
            self.copy
                .await
                .wrap_err("copy task panicked")?
                .wrap_err("error in copy task")
        };
        let summary_fut =
            async { self.summary.await.wrap_err("summary task panicked") };
        let processing_fut = async {
            if let Some(processing) = self.processing {
                processing
                    .await
                    .wrap_err("processing task panicked")?
                    .wrap_err("processing task errored")?;
            }
            Ok::<_, color_eyre::Report>(())
        };

        let ((), summary, ()) =
            tokio::try_join!(copy_fut, summary_fut, processing_fut)?;

        Ok(summary)
    }
}

pub async fn execute_plan(
    plan: &DiffPlan,
    cancel: CancellationToken,
) -> Result<DiffPlanOutputs> {
    let multi_bar = indicatif::MultiProgress::new();
    let mut summaries = HashMap::new();
    let mut tasks = Vec::new();
    for op in plan.ops.iter() {
        let (chunk_tx, chunk_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let summary_task = tokio::task::spawn_blocking(|| summary_task_entry(chunk_rx));
        let (size, op_tasks) = match op {
            Operation::Bidiff {
                id,
                old_path,
                new_path,
                out_path,
            } => {
                info!(
                    "executing diff operation - old: {old_path:?}, new: {new_path:?}, out: {out_path:?}"
                );
                let out_file =
                    tokio::fs::File::create_new(&out_path).await.wrap_err_with(
                        || format!("failed to create new out_path at `{out_path:?}`"),
                    )?;

                let (processed_tx, processed_rx) = mpsc::channel(CHANNEL_CAPACITY);
                let old_path = old_path.clone();
                let new_path = new_path.clone();
                let processing_task = tokio::task::spawn_blocking(move || {
                    bidiff_processing_entry(&old_path, &new_path, processed_tx)
                });
                let copy_task_cancel = cancel.child_token();
                let copy_task = tokio::task::spawn(async move {
                    copy_task_cancel
                        .run_until_cancelled(copy_task_entry(
                            // map needed to turn items into Results
                            ReceiverStream::new(processed_rx).map(Ok),
                            out_file,
                            chunk_tx,
                        ))
                        .await
                        .unwrap_or(Ok(()))
                });
                let op_tasks = OpTasks {
                    id: id.to_owned(),
                    copy: copy_task,
                    summary: summary_task,
                    processing: Some(processing_task),
                };
                let size = None; // Size is not known until bidifff completes
                (size, op_tasks)
            }
            Operation::Copy {
                id,
                from_path,
                to_path,
            } => {
                info!(
                    "executing copy operation - from: {from_path:?}, to: {to_path:?}"
                );
                let file_size = tokio::fs::metadata(&from_path)
                    .await
                    .wrap_err_with(|| {
                        format!("failed to read metadata of from_file `{from_path:?}`")
                    })?
                    .len();

                let from_file =
                    tokio::fs::File::open(from_path).await.wrap_err_with(|| {
                        format!("failed to read from_file `{from_path:?}`")
                    })?;
                let to_file = tokio::fs::File::create_new(to_path)
                    .await
                    .wrap_err_with(|| {
                        format!("failed to create to_file `{to_path:?}`")
                    })?;

                let copy_task_cancel = cancel.child_token();
                let copy_task = tokio::task::spawn(async move {
                    copy_task_cancel
                        .run_until_cancelled(copy_task_entry(
                            ReaderStream::new(BufReader::new(from_file)),
                            to_file,
                            chunk_tx,
                        ))
                        .await
                        .unwrap_or(Ok(()))
                });

                let op_tasks = OpTasks {
                    id: id.to_owned(),
                    copy: copy_task,
                    summary: summary_task,
                    processing: None,
                };

                (Some(file_size), op_tasks)
            }
        };

        tasks.push(op_tasks);

        let id = op.id();
        let pb = if let Some(size) = size {
            indicatif::ProgressBar::new(size)
        } else {
            indicatif::ProgressBar::no_length()
        }
        .with_style(crate::progress_bar_style())
        .with_message(id.0.to_owned());
        multi_bar.add(pb);
    }

    let task_results =
        futures::future::try_join_all(tasks.into_iter().map(|t| async {
            let id = t.id.clone();
            let summary = t.try_join().await?;
            Ok::<_, color_eyre::Report>((id, summary))
        }))
        .await?;
    for (id, summary) in task_results {
        summaries.insert(id, summary);
    }

    Ok(DiffPlanOutputs { summaries })
}

// blocking task
fn summary_task_entry(mut chunk_rx: mpsc::Receiver<Bytes>) -> FileSummary {
    let mut hasher = Sha256::new();
    let mut size = 0;
    while let Some(chunk) = chunk_rx.blocking_recv() {
        hasher.update(&chunk);
        size += u64::try_from(chunk.len()).expect("overflow");
    }

    let hash = hasher.finalize();
    let hash = Vec::from(hash.as_slice());

    FileSummary { hash, size }
}

// Async task
async fn copy_task_entry(
    from: impl Stream<Item = IoResult<Bytes>>,
    to: tokio::fs::File,
    summary_tx: mpsc::Sender<Bytes>,
) -> Result<()> {
    let mut to = tokio::io::BufWriter::new(to);
    let mut from = pin!(from);
    while let Some(chunk) = from.try_next().await? {
        if summary_tx.send(Bytes::clone(&chunk)).await.is_err() {
            // Summarizer closed, our task should just terminate cleanly to avoid
            // confusing errors.
            return Ok(());
        }
        to.write_all(chunk.as_ref()).await?;
    }
    to.flush().await?;
    to.into_inner().sync_all().await?;

    Ok(())
}

/// Helper struct to be able to make a [`mpsc::Sender<Bytes>`] implement
/// [`std::io::Write`].
struct ChannelAsWrite {
    tx: mpsc::Sender<Bytes>,
}

impl std::io::Write for ChannelAsWrite {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        let bytes = Bytes::copy_from_slice(buf);
        if let Err(err) = self.tx.blocking_send(bytes) {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, err));
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

// Blocking task
fn bidiff_processing_entry(
    old_path: &Path,
    new_path: &Path,
    processed_tx: mpsc::Sender<Bytes>,
) -> Result<()> {
    let out_writer = ChannelAsWrite { tx: processed_tx };
    let mut encoder =
        zstd::Encoder::new(out_writer, 0).expect("infallible: 0 should always work");

    // TODO: instead of reading the entire file, it may make sense to memmap large
    // files.
    let old_contents = std::fs::read(old_path)
        .wrap_err_with(|| format!("failed to read old_path at `{old_path:?}`"))?;
    let new_contents = std::fs::read(new_path)
        .wrap_err_with(|| format!("failed to read new_path at `{new_path:?}`"))?;
    orb_bidiff_squashfs::diff_squashfs()
        .old_path(old_path)
        .old(&old_contents)
        .new_path(new_path)
        .new(&new_contents)
        .out(&mut encoder)
        .diff_params(&DiffParams::default())
        .call()
        .wrap_err("failed to perform diff")?;

    Ok(())
}
