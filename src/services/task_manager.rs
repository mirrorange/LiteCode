use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio::sync::{Mutex, Notify, RwLock, oneshot};

use crate::{
    error::{LiteCodeError, Result},
    schema::{TaskOutputInput, TaskOutputResponse, TaskStopOutput},
};

#[derive(Clone, Debug, Default)]
pub struct TaskManager {
    inner: Arc<TaskManagerInner>,
}

#[derive(Debug, Default)]
struct TaskManagerInner {
    next_id: AtomicU64,
    tasks: RwLock<HashMap<String, Arc<TaskRecord>>>,
}

#[derive(Debug)]
struct TaskRecord {
    state: RwLock<TaskCompletion>,
    notify: Notify,
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

#[derive(Debug, Clone)]
pub struct TaskCompletion {
    pub status: String,
    pub stdout: String,
    pub stderr: String,
    pub interrupted: bool,
    pub completed: bool,
}

impl TaskCompletion {
    pub fn running() -> Self {
        Self {
            status: "running".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            interrupted: false,
            completed: false,
        }
    }

    pub fn failed(message: String) -> Self {
        Self {
            status: "failed".to_string(),
            stdout: String::new(),
            stderr: message,
            interrupted: false,
            completed: true,
        }
    }
}

impl TaskManager {
    pub async fn register_shell_task(&self) -> String {
        let id = format!(
            "task-{}",
            self.inner.next_id.fetch_add(1, Ordering::Relaxed) + 1
        );
        let (stop_tx, _stop_rx) = oneshot::channel::<()>();

        let task = Arc::new(TaskRecord {
            state: RwLock::new(TaskCompletion::running()),
            notify: Notify::new(),
            stop_tx: Mutex::new(Some(stop_tx)),
        });

        self.inner.tasks.write().await.insert(id.clone(), task);
        id
    }

    pub async fn subscribe_stop(&self, task_id: &str) -> Result<oneshot::Receiver<()>> {
        let task = self.get_task(task_id).await?;
        let mut stop_guard = task.stop_tx.lock().await;
        let (stop_tx, stop_rx) = oneshot::channel();
        *stop_guard = Some(stop_tx);
        Ok(stop_rx)
    }

    pub async fn finish_task(&self, task_id: &str, completion: TaskCompletion) {
        if let Some(task) = self.inner.tasks.read().await.get(task_id).cloned() {
            *task.state.write().await = completion;
            task.notify.notify_waiters();
        }
    }

    pub async fn task_output(&self, input: TaskOutputInput) -> Result<TaskOutputResponse> {
        let task = self.get_task(&input.task_id).await?;

        if input.block {
            let is_complete = { task.state.read().await.completed };
            if !is_complete {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(input.timeout.min(600_000)),
                    task.notify.notified(),
                )
                .await;
            }
        }

        let state = task.state.read().await.clone();
        Ok(TaskOutputResponse {
            status: state.status,
            stdout: state.stdout,
            stderr: state.stderr,
        })
    }

    pub async fn stop_task(&self, task_id: &str) -> Result<TaskStopOutput> {
        let task = self.get_task(task_id).await?;
        let current = task.state.read().await.clone();
        if current.completed {
            return Ok(TaskStopOutput {
                message: "Task already completed.".to_string(),
            });
        }

        let mut stop_guard = task.stop_tx.lock().await;
        if let Some(stop_tx) = stop_guard.take() {
            let _ = stop_tx.send(());
        }

        Ok(TaskStopOutput {
            message: "Stop signal sent.".to_string(),
        })
    }

    async fn get_task(&self, task_id: &str) -> Result<Arc<TaskRecord>> {
        self.inner
            .tasks
            .read()
            .await
            .get(task_id)
            .cloned()
            .ok_or_else(|| LiteCodeError::invalid_input(format!("Unknown task id {task_id}.")))
    }
}
