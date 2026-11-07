use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

const MAX_OUTPUT_LINES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub id: String,
    pub command: String,
    pub tool_summary: Option<String>,
    pub is_daemon: bool,
    pub status: String, // "running" | "completed" | "failed" | "killed"
    pub pid: Option<u32>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub log_path: String,
    pub recent_output: String,
}

struct TaskEntry {
    info: TaskInfo,
    output_lines: VecDeque<String>,
    stdin_tx: Option<mpsc::Sender<String>>,
    cancel: CancellationToken,
}

pub struct TaskManager {
    tasks: RwLock<HashMap<String, Arc<RwLock<TaskEntry>>>>,
    counter: AtomicU64,
}

static GLOBAL_TASK_MANAGER: OnceLock<TaskManager> = OnceLock::new();

pub fn get_task_manager() -> &'static TaskManager {
    GLOBAL_TASK_MANAGER.get_or_init(TaskManager::new)
}

fn get_tasks_dir() -> PathBuf {
    let base = dirs::home_dir()
        .map(|h| h.join(".aether-agent"))
        .unwrap_or_else(|| PathBuf::from(".aether-agent"));
    let tasks_dir = base.join("tasks");
    let _ = std::fs::create_dir_all(&tasks_dir);
    tasks_dir
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            counter: AtomicU64::new(1),
        }
    }

    pub async fn spawn_task(
        &self,
        command: &str,
        cwd: &str,
        tool_summary: Option<String>,
        is_daemon: bool,
    ) -> Result<TaskInfo, String> {
        let task_num = self.counter.fetch_add(1, Ordering::SeqCst);
        let task_id = format!("task_{task_num}");
        let tasks_dir = get_tasks_dir();
        let log_file_path = tasks_dir.join(format!("{task_id}.log"));
        let log_path_str = log_file_path.to_string_lossy().to_string();

        let run_dir = if cwd.trim().is_empty() {
            std::env::temp_dir()
        } else {
            PathBuf::from(cwd)
        };

        let mut cmd = if cfg!(windows) {
            let mut p = super::create_hidden_command("powershell.exe");
            p.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                command,
            ]);
            p
        } else {
            let mut p = super::create_hidden_command("sh");
            p.args(["-c", command]);
            p
        };

        cmd.current_dir(run_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .kill_on_drop(false);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn background task: {e}"))?;

        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdin = child.stdin.take();

        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(32);
        if let Some(mut child_stdin) = stdin {
            tokio::spawn(async move {
                while let Some(msg) = stdin_rx.recv().await {
                    if child_stdin.write_all(msg.as_bytes()).await.is_err() {
                        break;
                    }
                    if !msg.ends_with('\n') {
                        let _ = child_stdin.write_all(b"\n").await;
                    }
                    if child_stdin.flush().await.is_err() {
                        break;
                    }
                }
            });
        }

        let cancel = CancellationToken::new();
        let now = chrono::Utc::now().timestamp_millis();

        let info = TaskInfo {
            id: task_id.clone(),
            command: command.to_string(),
            tool_summary: tool_summary.clone(),
            is_daemon,
            status: "running".to_string(),
            pid,
            started_at: now,
            completed_at: None,
            exit_code: None,
            log_path: log_path_str.clone(),
            recent_output: String::new(),
        };

        let entry = Arc::new(RwLock::new(TaskEntry {
            info: info.clone(),
            output_lines: VecDeque::with_capacity(MAX_OUTPUT_LINES),
            stdin_tx: Some(stdin_tx),
            cancel: cancel.clone(),
        }));

        {
            let mut map = self.tasks.write().await;
            map.insert(task_id.clone(), entry.clone());
        }

        // Spawn output streaming & process monitoring task
        let monitor_entry = entry.clone();
        let monitor_cancel = cancel.clone();
        let log_file_for_writer = log_file_path.clone();

        tokio::spawn(async move {
            let (line_tx, mut line_rx) = mpsc::channel::<(String, String)>(128);

            if let Some(out) = stdout {
                let tx = line_tx.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(out).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        if tx.send(("stdout".into(), line)).await.is_err() {
                            break;
                        }
                    }
                });
            }

            if let Some(err) = stderr {
                let tx = line_tx.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(err).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        if tx.send(("stderr".into(), line)).await.is_err() {
                            break;
                        }
                    }
                });
            }
            drop(line_tx);

            // Open log file for appending
            let mut log_file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file_for_writer)
                .await
                .ok();

            let mut process_exited = false;
            let mut final_exit_code: Option<i32> = None;

            loop {
                tokio::select! {
                    biased;

                    _ = monitor_cancel.cancelled() => {
                        let _ = child.kill().await;
                        break;
                    }

                    maybe_line = line_rx.recv() => {
                        match maybe_line {
                            Some((_stream, line)) => {
                                if let Some(f) = &mut log_file {
                                    let _ = f.write_all(format!("{line}\n").as_bytes()).await;
                                    let _ = f.flush().await;
                                }
                                let mut guard = monitor_entry.write().await;
                                if guard.output_lines.len() >= MAX_OUTPUT_LINES {
                                    guard.output_lines.pop_front();
                                }
                                guard.output_lines.push_back(line);
                            }
                            None => {
                                // Channel closed, wait for process exit
                                if !process_exited {
                                    match child.wait().await {
                                        Ok(status) => {
                                            final_exit_code = status.code();
                                        }
                                        Err(_) => {
                                            final_exit_code = Some(-1);
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }

                    status_res = child.wait(), if !process_exited => {
                        process_exited = true;
                        match status_res {
                            Ok(status) => {
                                final_exit_code = status.code();
                            }
                            Err(_) => {
                                final_exit_code = Some(-1);
                            }
                        }
                    }
                }
            }

            // Drain remaining lines if any
            while let Ok((_stream, line)) = line_rx.try_recv() {
                if let Some(f) = &mut log_file {
                    let _ = f.write_all(format!("{line}\n").as_bytes()).await;
                }
                let mut guard = monitor_entry.write().await;
                if guard.output_lines.len() >= MAX_OUTPUT_LINES {
                    guard.output_lines.pop_front();
                }
                guard.output_lines.push_back(line);
            }

            let end_time = chrono::Utc::now().timestamp_millis();
            let mut guard = monitor_entry.write().await;
            guard.stdin_tx = None;
            guard.info.completed_at = Some(end_time);
            guard.info.exit_code = final_exit_code;

            if guard.info.status != "killed" {
                if final_exit_code == Some(0) {
                    guard.info.status = "completed".to_string();
                } else {
                    guard.info.status = "failed".to_string();
                }
            }
            guard.info.recent_output = guard.output_lines.iter().cloned().collect::<Vec<_>>().join("\n");
        });

        Ok(info)
    }

    pub async fn list_tasks(&self) -> Vec<TaskInfo> {
        let map = self.tasks.read().await;
        let mut list = Vec::new();
        for entry in map.values() {
            let guard = entry.read().await;
            let mut info = guard.info.clone();
            info.recent_output = guard.output_lines.iter().cloned().collect::<Vec<_>>().join("\n");
            list.push(info);
        }
        list.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        list
    }

    pub async fn get_status(&self, task_id: &str, tail_lines: Option<usize>) -> Result<TaskInfo, String> {
        let entry_arc = {
            let map = self.tasks.read().await;
            map.get(task_id).cloned().ok_or_else(|| format!("Task '{task_id}' not found"))?
        };
        let guard = entry_arc.read().await;
        let mut info = guard.info.clone();
        let limit = tail_lines.unwrap_or(50).clamp(1, MAX_OUTPUT_LINES);
        let count = guard.output_lines.len();
        let skip = count.saturating_sub(limit);
        info.recent_output = guard
            .output_lines
            .iter()
            .skip(skip)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        Ok(info)
    }

    pub async fn send_input(&self, task_id: &str, input: &str) -> Result<String, String> {
        let entry_arc = {
            let map = self.tasks.read().await;
            map.get(task_id).cloned().ok_or_else(|| format!("Task '{task_id}' not found"))?
        };
        let guard = entry_arc.read().await;
        if guard.info.status != "running" {
            return Err(format!("Task '{task_id}' is not running (status: {})", guard.info.status));
        }
        if let Some(tx) = &guard.stdin_tx {
            tx.send(input.to_string())
                .await
                .map_err(|_| "Failed to deliver input to task stdin".to_string())?;
            Ok(format!("Input delivered to task '{task_id}'"))
        } else {
            Err(format!("Task '{task_id}' stdin is closed"))
        }
    }

    pub async fn kill_task(&self, task_id: &str) -> Result<String, String> {
        let entry_arc = {
            let map = self.tasks.read().await;
            map.get(task_id).cloned().ok_or_else(|| format!("Task '{task_id}' not found"))?
        };
        let (pid, cancel) = {
            let mut guard = entry_arc.write().await;
            if guard.info.status != "running" {
                return Ok(format!("Task '{task_id}' is already {}", guard.info.status));
            }
            guard.info.status = "killed".to_string();
            (guard.info.pid, guard.cancel.clone())
        };

        // Terminate process tree on Windows
        #[cfg(windows)]
        if let Some(p) = pid {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &p.to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .output();
        }

        cancel.cancel();
        Ok(format!("Task '{task_id}' terminated"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_complete_task() {
        let mgr = TaskManager::new();
        let cmd = if cfg!(windows) {
            "Write-Output 'task completed successfully'"
        } else {
            "echo 'task completed successfully'"
        };

        let info = mgr
            .spawn_task(cmd, "", Some("Test task".into()), false)
            .await
            .expect("spawn task");

        assert_eq!(info.status, "running");
        assert!(info.id.starts_with("task_"));

        // Wait for completion (give enough headroom under parallel test load)
        let mut completed = false;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let status = mgr.get_status(&info.id, Some(10)).await.expect("get status");
            if status.status == "completed" {
                completed = true;
                assert!(status.recent_output.contains("task completed successfully"));
                break;
            }
        }
        assert!(completed, "task should have completed");
    }

    #[tokio::test]
    async fn test_kill_running_task() {
        let mgr = TaskManager::new();
        let cmd = if cfg!(windows) {
            "Start-Sleep -Seconds 30"
        } else {
            "sleep 30"
        };

        let info = mgr
            .spawn_task(cmd, "", Some("Sleep task".into()), true)
            .await
            .expect("spawn sleep task");

        assert_eq!(info.status, "running");
        assert!(info.is_daemon);

        // Kill the task
        let kill_res = mgr.kill_task(&info.id).await.expect("kill task");
        assert!(kill_res.contains("terminated"));

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let status = mgr.get_status(&info.id, None).await.expect("status after kill");
        assert_eq!(status.status, "killed");
    }

    #[tokio::test]
    async fn test_task_not_found() {
        let mgr = TaskManager::new();
        let err = mgr.get_status("nonexistent_task", None).await.unwrap_err();
        assert!(err.contains("not found"));
    }
}
