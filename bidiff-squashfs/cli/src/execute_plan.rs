use std::pin::pin;
use std::time::Duration;
use std::{io::Result as IoResult, path::Path};

use bytes::Bytes;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::Stream;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt as _;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;
use tracing::info;

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

/// The three potential types of tasks involved in executing an operation.
struct OpTasks {
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
    let _cancel_guard = cancel.clone().drop_guard();
    let multi_bar = indicatif::MultiProgress::new();
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
                let out_file =
                    tokio::fs::File::create_new(&out_path).await.wrap_err_with(
                        || format!("failed to create new out_path at `{out_path:?}`"),
                    )?;

                let (processed_tx, processed_rx) = mpsc::channel(CHANNEL_CAPACITY);
                let processing_cancel = cancel.child_token();
                let old_path = old_path.clone();
                let new_path = new_path.clone();
                let out_path = out_path.clone();
                let processing_task = tokio::task::spawn_blocking(move || {
                    bidiff_processing_entry(
                        &old_path,
                        &new_path,
                        &out_path,
                        processed_tx,
                        processing_cancel,
                    )
                });
                let copy_task = tokio::task::spawn(copy_task_entry(
                    // map needed to turn items into Results
                    ReceiverStream::new(processed_rx).map(|v| Ok(v)),
                    BufWriter::new(out_file),
                    chunk_tx,
                ));
                let op_tasks = OpTasks {
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
                let mdata =
                    tokio::fs::metadata(&from_path).await.wrap_err_with(|| {
                        format!("failed to read metadata of from_file `{from_path:?}`")
                    })?;

                let from_file =
                    tokio::fs::File::open(from_path).await.wrap_err_with(|| {
                        format!("failed to read from_file `{from_path:?}`")
                    })?;
                let to_file = tokio::fs::File::create_new(to_path)
                    .await
                    .wrap_err_with(|| {
                        format!("failed to create to_file `{to_path:?}`")
                    })?;

                let copy_task = tokio::task::spawn(copy_task_entry(
                    ReaderStream::new(BufReader::new(from_file)),
                    BufWriter::new(to_file),
                    chunk_tx,
                ));

                let op_tasks = OpTasks {
                    copy: copy_task,
                    summary: summary_task,
                    processing: None,
                };
                let size = Some(mdata.len()); // Size is known from file size

                (size, op_tasks)
            }
        };

        let pb = if let Some(size) = size {
            indicatif::ProgressBar::new(size)
        } else {
            indicatif::ProgressBar::no_length()
        }
        .with_style(crate::progress_bar_style())
        .with_message(op.id().0.to_owned());
        multi_bar.add(pb);

        let summary = op_tasks.try_join().await?; // TODO: allow ops to process concurrently
        info!("summary: {summary:?}");
    }

    todo!()
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
    to: impl AsyncWrite,
    summary_tx: mpsc::Sender<Bytes>,
) -> Result<()> {
    let mut from = pin!(from);
    let mut to = pin!(to);
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

/// Helper struct to be able to "write" to a channel.
struct DiffWriter {
    tx: mpsc::Sender<Bytes>,
    cancel: CancellationToken,
}

impl std::io::Write for DiffWriter {
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
    _old_path: &Path,
    _new_path: &Path,
    _out_path: &Path,
    processed_tx: mpsc::Sender<Bytes>,
    cancel: CancellationToken,
) -> Result<()> {
    let _diff_writer = DiffWriter {
        tx: processed_tx,
        cancel,
    };
    todo!("call bidiff")
}
