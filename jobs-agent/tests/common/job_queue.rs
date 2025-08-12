#![allow(dead_code)]
use orb_relay_messages::jobs::v1::JobExecution;
use test_utils::async_bag::AsyncBag;

/// A double for the job queue handling implemented on fleet-cmdr.
///
/// Cheap to clone (`AsyncBag` uses an `Arc` underneath).
#[derive(Default, Clone, Debug)]
pub struct JobQueue {
    val: AsyncBag<Vec<QueuedJob>>,
}

#[derive(Debug)]
pub struct QueuedJob {
    exec: JobExecution,
    /// Used to acknowledge job has been handled
    ack_tx: flume::Sender<()>,
}

/// Ticket given once a job is enqueued.
#[derive(Debug)]
pub struct Ticket {
    pub exec_id: String,
    ack_rx: flume::Receiver<()>,
}

impl Ticket {
    pub async fn wait_for_completion(&self) {
        self.ack_rx.recv_async().await.unwrap();
    }
}

impl JobQueue {
    pub async fn enqueue(&self, exec: JobExecution) -> Ticket {
        let (ack_tx, ack_rx) = flume::unbounded();

        let ticket = Ticket {
            exec_id: exec.job_execution_id.clone(),
            ack_rx,
        };

        let queued_job = QueuedJob { exec, ack_tx };

        self.val.lock().await.push(queued_job);

        ticket
    }

    pub async fn next(&self, exec_ids_to_ignore: &[String]) -> Option<JobExecution> {
        self.val
            .lock()
            .await
            .iter()
            .find(|j| !exec_ids_to_ignore.contains(&j.exec.job_execution_id))
            .map(|j| &j.exec)
            .cloned()
    }

    pub async fn handled(&self, job_exec_id: impl Into<String>) {
        let job_exec_id = job_exec_id.into();

        let mut val = self.val.lock().await;
        let Some(pos) = val
            .iter()
            .position(|j| j.exec.job_execution_id == job_exec_id)
        else {
            return;
        };

        let removed_job = val.remove(pos);
        removed_job.ack_tx.send(()).unwrap();
    }
}
