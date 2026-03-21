use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{io::AsyncReadExt, process::Command, sync::oneshot, time::timeout};

use crate::{
    error::{LiteCodeError, Result},
    schema::{BashInput, BashOutput},
    services::task_manager::{TaskCompletion, TaskManager},
};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const PWD_MARKER: &str = "__LITECODE_PWD__:";

#[derive(Clone, Debug)]
pub struct ProcessService {
    working_dir: Arc<Mutex<PathBuf>>,
}

impl ProcessService {
    pub fn new(working_dir: Arc<Mutex<PathBuf>>) -> Self {
        Self { working_dir }
    }

    pub fn working_dir(&self) -> PathBuf {
        self.working_dir
            .lock()
            .expect("working directory lock poisoned")
            .clone()
    }

    pub async fn bash(&self, input: BashInput, task_manager: &TaskManager) -> Result<BashOutput> {
        if input.command.trim().is_empty() {
            return Err(LiteCodeError::invalid_input(
                "Bash command cannot be empty.",
            ));
        }

        let timeout_ms = input
            .timeout
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);

        if input.run_in_background.unwrap_or(false) {
            self.spawn_background(input, timeout_ms, task_manager).await
        } else {
            self.run_foreground(&input.command, timeout_ms).await
        }
    }

    async fn run_foreground(&self, command: &str, timeout_ms: u64) -> Result<BashOutput> {
        let child = self.spawn_shell(command)?;
        let outcome = wait_for_child(child, None, Duration::from_millis(timeout_ms)).await?;
        if let Some(path) = outcome.final_working_dir {
            self.set_working_dir(path);
        }

        Ok(BashOutput {
            stdout: Some(outcome.stdout),
            stderr: Some(outcome.stderr),
            interrupted: Some(outcome.interrupted),
            background_task_id: None,
        })
    }

    async fn spawn_background(
        &self,
        input: BashInput,
        timeout_ms: u64,
        task_manager: &TaskManager,
    ) -> Result<BashOutput> {
        let child = self.spawn_shell(&input.command)?;
        let task_id = task_manager.register_shell_task().await;

        let working_dir = self.working_dir.clone();
        let manager = task_manager.clone();
        let stop_rx = manager.subscribe_stop(&task_id).await?;
        let task_id_for_worker = task_id.clone();

        tokio::spawn(async move {
            let completion =
                match wait_for_child(child, Some(stop_rx), Duration::from_millis(timeout_ms)).await
                {
                    Ok(outcome) => {
                        if let Some(path) = outcome.final_working_dir {
                            *working_dir.lock().expect("working directory lock poisoned") = path;
                        }
                        TaskCompletion {
                            status: if outcome.interrupted {
                                "stopped".to_string()
                            } else {
                                "completed".to_string()
                            },
                            stdout: outcome.stdout,
                            stderr: outcome.stderr,
                            interrupted: outcome.interrupted,
                            completed: true,
                        }
                    }
                    Err(error) => TaskCompletion::failed(error.to_string()),
                };

            manager.finish_task(&task_id_for_worker, completion).await;
        });

        Ok(BashOutput {
            stdout: None,
            stderr: None,
            interrupted: None,
            background_task_id: Some(task_id),
        })
    }

    fn spawn_shell(&self, command: &str) -> Result<tokio::process::Child> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let mut process = Command::new(shell);
        process
            .arg("-lc")
            .arg(wrap_command(command))
            .current_dir(self.working_dir())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        process.spawn().map_err(Into::into)
    }

    fn set_working_dir(&self, path: impl AsRef<Path>) {
        *self
            .working_dir
            .lock()
            .expect("working directory lock poisoned") = path.as_ref().to_path_buf();
    }
}

#[derive(Debug)]
struct ProcessOutcome {
    stdout: String,
    stderr: String,
    interrupted: bool,
    final_working_dir: Option<PathBuf>,
}

async fn wait_for_child(
    mut child: tokio::process::Child,
    mut stop_rx: Option<oneshot::Receiver<()>>,
    duration: Duration,
) -> Result<ProcessOutcome> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LiteCodeError::internal("Child stdout pipe was not configured."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LiteCodeError::internal("Child stderr pipe was not configured."))?;

    let stdout_handle = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).await.map(|_| buffer)
    });
    let stderr_handle = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).await.map(|_| buffer)
    });

    let wait_future = async {
        if let Some(receiver) = stop_rx.as_mut() {
            tokio::select! {
                result = child.wait() => result.map(|status| (status, false)),
                _ = receiver => {
                    let _ = child.kill().await;
                    child.wait().await.map(|status| (status, true))
                }
            }
        } else {
            child.wait().await.map(|status| (status, false))
        }
    };

    let (_status, interrupted) = match timeout(duration, wait_future).await {
        Ok(result) => result?,
        Err(_) => {
            let _ = child.kill().await;
            (child.wait().await?, true)
        }
    };

    let stdout = String::from_utf8_lossy(&stdout_handle.await??).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_handle.await??).into_owned();
    let (stdout, final_working_dir) = extract_working_dir(&stdout);

    Ok(ProcessOutcome {
        stdout,
        stderr,
        interrupted,
        final_working_dir,
    })
}

fn wrap_command(command: &str) -> String {
    format!(
        "{command}; __litecode_status=$?; printf '\\n{PWD_MARKER}%s' \"$PWD\"; exit $__litecode_status"
    )
}

fn extract_working_dir(stdout: &str) -> (String, Option<PathBuf>) {
    match stdout.rfind(PWD_MARKER) {
        Some(index) => {
            let path = stdout[index + PWD_MARKER.len()..].trim();
            let cleaned = stdout[..index].trim_end_matches('\n').to_string();
            (cleaned, (!path.is_empty()).then(|| PathBuf::from(path)))
        }
        None => (stdout.to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{Value, to_value};
    use tempfile::tempdir;

    use crate::{
        schema::{BashInput, TaskOutputInput},
        services::task_manager::TaskManager,
    };

    use super::ProcessService;

    #[tokio::test]
    async fn bash_updates_working_directory() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        let service = ProcessService::new(Arc::new(Mutex::new(dir.path().to_path_buf())));
        let tasks = TaskManager::default();
        let output = service
            .bash(
                BashInput {
                    command: format!("cd \"{}\" && pwd", nested.display()),
                    timeout: Some(5_000),
                    description: None,
                    run_in_background: Some(false),
                },
                &tasks,
            )
            .await
            .unwrap();

        assert!(
            output
                .stdout
                .as_deref()
                .unwrap()
                .contains(nested.to_string_lossy().as_ref())
        );
        assert_eq!(service.working_dir(), nested);

        let keys = object_keys(&to_value(&output).unwrap());
        assert_eq!(keys, vec!["interrupted", "stderr", "stdout"]);
    }

    #[tokio::test]
    async fn background_task_can_be_polled() {
        let dir = tempdir().unwrap();
        let service = ProcessService::new(Arc::new(Mutex::new(dir.path().to_path_buf())));
        let tasks = TaskManager::default();

        let output = service
            .bash(
                BashInput {
                    command: "sleep 1 && echo ready".to_string(),
                    timeout: Some(5_000),
                    description: None,
                    run_in_background: Some(true),
                },
                &tasks,
            )
            .await
            .unwrap();

        let keys = object_keys(&to_value(&output).unwrap());
        assert_eq!(keys, vec!["backgroundTaskId"]);

        let task_id = output.background_task_id.unwrap();
        let status = tasks
            .task_output(TaskOutputInput {
                task_id,
                block: true,
                timeout: 5_000,
            })
            .await
            .unwrap();

        assert_eq!(status.status, "completed");
        assert!(status.stdout.contains("ready"));

        let keys = object_keys(&to_value(&status).unwrap());
        assert_eq!(keys, vec!["status", "stderr", "stdout"]);
    }

    #[tokio::test]
    async fn background_task_can_be_stopped() {
        let dir = tempdir().unwrap();
        let service = ProcessService::new(Arc::new(Mutex::new(dir.path().to_path_buf())));
        let tasks = TaskManager::default();

        let output = service
            .bash(
                BashInput {
                    command: "sleep 30".to_string(),
                    timeout: Some(60_000),
                    description: Some("Long sleep".to_string()),
                    run_in_background: Some(true),
                },
                &tasks,
            )
            .await
            .unwrap();

        let task_id = output.background_task_id.unwrap();
        let stop = tasks.stop_task(&task_id).await.unwrap();
        assert_eq!(stop.message, "Stop signal sent.");
        let keys = object_keys(&to_value(&stop).unwrap());
        assert_eq!(keys, vec!["message"]);

        let status = tasks
            .task_output(TaskOutputInput {
                task_id,
                block: true,
                timeout: 5_000,
            })
            .await
            .unwrap();

        assert_eq!(status.status, "stopped");
        let keys = object_keys(&to_value(&status).unwrap());
        assert_eq!(keys, vec!["status", "stderr", "stdout"]);
    }

    fn object_keys(value: &Value) -> Vec<String> {
        let mut keys = value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }
}
