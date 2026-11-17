use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

const MAX_OUTPUT_LINES: usize = 500;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const OUTPUT_CHUNK_BYTES: usize = 4096;
const MAX_LOG_BYTES: usize = 1024 * 1024;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const INPUT_TIMEOUT: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
const KILL_TIMEOUT: Duration = Duration::from_secs(15);
const LOG_TRUNCATED: &str = "\n[task log quota reached; further output omitted]\n";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub id: String,
    pub session_id: String,
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
    // Bounded line fragments; a newline-free producer cannot grow a single element.
    output_lines: VecDeque<String>,
    stdin_tx: Option<mpsc::Sender<String>>,
    cancel: CancellationToken,
    monitor_error: Option<String>,
    output_closed: bool,
}

impl TaskEntry {
    fn push_output(&mut self, text: &str) {
        for line in text.split_inclusive('\n') {
            let mut remaining = line;
            while !remaining.is_empty() {
                let end = char_boundary(remaining, OUTPUT_CHUNK_BYTES);
                self.output_lines.push_back(remaining[..end].to_owned());
                remaining = &remaining[end..];
            }
        }
        // At most 500 bounded fragments; count bytes, not Unicode scalar values.
        let mut bytes: usize = self.output_lines.iter().map(String::len).sum();
        while self.output_lines.len() > MAX_OUTPUT_LINES || bytes > MAX_OUTPUT_BYTES {
            if let Some(old) = self.output_lines.pop_front() {
                bytes -= old.len();
            }
        }
    }

    fn output(&self, limit: usize) -> String {
        self.output_lines
            .iter()
            .skip(self.output_lines.len().saturating_sub(limit))
            .map(String::as_str)
            .collect()
    }
}

fn char_boundary(text: &str, limit: usize) -> usize {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

pub struct TaskManager {
    tasks: RwLock<HashMap<String, Arc<RwLock<TaskEntry>>>>,
    log_dir: PathBuf,
    shutting_down: AtomicBool,
}

static GLOBAL_TASK_MANAGER: OnceLock<TaskManager> = OnceLock::new();

pub fn get_task_manager() -> &'static TaskManager {
    GLOBAL_TASK_MANAGER.get_or_init(TaskManager::new)
}

fn get_tasks_dir() -> PathBuf {
    #[cfg(not(test))]
    let base = dirs::home_dir()
        .map(|h| h.join(".aether-agent"))
        .unwrap_or_else(|| PathBuf::from(".aether-agent"));
    #[cfg(test)]
    let base = std::env::temp_dir()
        .join("aether-task-regression")
        .join(uuid::Uuid::new_v4().to_string());
    base.join("tasks")
}

async fn create_log(root: &Path, id: &str) -> Result<(PathBuf, tokio::fs::File), String> {
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|e| format!("Failed to prepare task log directory: {e}"))?;
    let path = root.join(format!("{id}.log"));
    let file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .await
        .map_err(|e| format!("Failed to create task log '{}': {e}", path.display()))?;
    Ok((path, file))
}

struct TaskLog<W> {
    writer: W,
    bytes: usize,
    truncated: bool,
}

impl<W: AsyncWrite + Unpin> TaskLog<W> {
    async fn write(&mut self, text: &str) -> Result<(), String> {
        if self.truncated {
            return Ok(());
        }
        let available = (MAX_LOG_BYTES - LOG_TRUNCATED.len()).saturating_sub(self.bytes);
        let end = char_boundary(text, available);
        tokio::time::timeout(IO_TIMEOUT, async {
            self.writer.write_all(&text.as_bytes()[..end]).await?;
            self.bytes += end;
            if end < text.len() {
                self.writer.write_all(LOG_TRUNCATED.as_bytes()).await?;
                self.bytes += LOG_TRUNCATED.len();
                self.truncated = true;
            }
            self.writer.flush().await
        })
        .await
        .map_err(|_| "Task log write timed out".to_string())?
        .map_err(|e| format!("Task log write failed: {e}"))
    }
}

// Keep incomplete UTF-8 only (at most three bytes). Invalid bytes are replaced visibly,
// rather than silently closing a reader; valid characters spanning reads are preserved.
fn decode_chunk(pending: &mut Vec<u8>, bytes: &[u8], eof: bool) -> String {
    pending.extend_from_slice(bytes);
    let mut output = String::new();
    let mut consumed = 0;
    while consumed < pending.len() {
        match std::str::from_utf8(&pending[consumed..]) {
            Ok(text) => {
                output.push_str(text);
                consumed = pending.len();
            }
            Err(error) => {
                let valid = consumed + error.valid_up_to();
                output.push_str(
                    std::str::from_utf8(&pending[consumed..valid]).expect("validated UTF-8 prefix"),
                );
                consumed = valid;
                if let Some(invalid) = error.error_len() {
                    output.push('\u{fffd}');
                    consumed += invalid;
                } else if eof {
                    output.push('\u{fffd}');
                    consumed = pending.len();
                } else {
                    break;
                }
            }
        }
    }
    pending.drain(..consumed);
    output
}

async fn read_output<R: AsyncRead + Unpin>(mut reader: R, tx: mpsc::Sender<String>, name: &str) {
    let mut bytes = [0; OUTPUT_CHUNK_BYTES];
    let mut pending = Vec::with_capacity(OUTPUT_CHUNK_BYTES + 3);
    loop {
        let (text, eof) = match reader.read(&mut bytes).await {
            Ok(n) => (decode_chunk(&mut pending, &bytes[..n], n == 0), n == 0),
            Err(error) => (format!("\n[task {name} read failed: {error}]\n"), true),
        };
        if !text.is_empty() && tx.send(text).await.is_err() {
            break;
        }
        if eof {
            break;
        }
    }
}

async fn record_output(
    entry: &Arc<RwLock<TaskEntry>>,
    log: &mut Option<TaskLog<tokio::fs::File>>,
    text: &str,
) {
    if let Some(writer) = log {
        if let Err(error) = writer.write(text).await {
            *log = None;
            let mut guard = entry.write().await;
            guard.push_output(&format!("\n[{error}]\n"));
            guard.monitor_error = Some(error);
        }
    }
    entry.write().await.push_output(text);
}

// The monitor alone owns/reaps Child and targets its still-live PID. No caller can
// taskkill a cached PID after wait() has retired it. Job Objects remain a separate policy.
async fn terminate_child(child: &mut Child) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|e| format!("Failed to inspect task: {e}"))?
        .is_some()
    {
        return Ok(());
    }
    #[cfg(windows)]
    let mut tree_error = None;
    #[cfg(not(windows))]
    let tree_error: Option<String> = None;
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let mut command = super::create_hidden_command("taskkill.exe");
        command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .kill_on_drop(true);
        match tokio::time::timeout(Duration::from_secs(8), command.output()).await {
            Ok(Ok(result)) if result.status.success() => {}
            Ok(Ok(result)) => {
                tree_error = Some(format!(
                    "Task tree termination failed: {}",
                    String::from_utf8_lossy(&result.stderr).trim()
                ))
            }
            Ok(Err(error)) => tree_error = Some(format!("Task tree termination failed: {error}")),
            Err(_) => tree_error = Some("Task tree termination timed out".into()),
        }
    }
    tokio::time::timeout(Duration::from_secs(3), child.kill())
        .await
        .map_err(|_| "Task process termination timed out".to_string())?
        .map_err(|e| format!("Task process termination failed: {e}"))?;
    if let Some(error) = tree_error {
        return Err(error);
    }
    Ok(())
}

async fn monitor_task(
    mut child: Child,
    entry: Arc<RwLock<TaskEntry>>,
    file: tokio::fs::File,
    stdin_worker: tokio::task::JoinHandle<()>,
) {
    let cancel = entry.read().await.cancel.clone();
    let (output_tx, mut output_rx) = mpsc::channel::<String>(32);
    let mut workers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        workers.push(tokio::spawn(read_output(
            stdout,
            output_tx.clone(),
            "stdout",
        )));
    }
    if let Some(stderr) = child.stderr.take() {
        workers.push(tokio::spawn(read_output(
            stderr,
            output_tx.clone(),
            "stderr",
        )));
    }
    drop(output_tx);
    let mut log = Some(TaskLog {
        writer: file,
        bytes: 0,
        truncated: false,
    });
    let mut output_closed = false;
    let mut process_exited = false;
    let mut killed = false;
    let mut cancellation_handled = false;
    let mut final_exit_code = None;
    let mut drain_deadline = tokio::time::Instant::now();
    // stdin is started separately by start_task; dropping its sender at exit plus
    // cancellation/abort below deterministically closes it even during write_all.
    loop {
        if output_closed && process_exited {
            break;
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled(), if !cancellation_handled => {
                cancellation_handled = true;
                if !process_exited {
                    killed = true;
                    if let Err(error) = terminate_child(&mut child).await {
                        record_output(&entry, &mut log, &format!("\n[{error}]\n")).await;
                        entry.write().await.monitor_error = Some(error);
                    }
                    final_exit_code = child.try_wait().ok().flatten().and_then(|s| s.code());
                    if !matches!(child.try_wait(), Ok(Some(_))) {
                        // Failed termination is NOT completion. Keep owning/polling
                        // the child until it really exits; never retire a live PID.
                        continue;
                    }
                }
                break;
            }
            status = child.wait(), if !process_exited => {
                process_exited = true;
                final_exit_code = match status {
                    Ok(status) => status.code(),
                    Err(error) => {
                        let error = format!("Task wait failed: {error}");
                        record_output(&entry, &mut log, &format!("\n[{error}]\n")).await;
                        entry.write().await.monitor_error = Some(error);
                        Some(-1)
                    }
                };
                entry.write().await.info.pid = None;
                drain_deadline = tokio::time::Instant::now() + DRAIN_TIMEOUT;
            }
            _ = tokio::time::sleep_until(drain_deadline), if process_exited => {
                record_output(&entry, &mut log, "\n[task output drain deadline reached; inherited pipes closed]\n").await;
                break;
            }
            chunk = output_rx.recv(), if !output_closed => {
                match chunk {
                    Some(text) => record_output(&entry, &mut log, &text).await,
                    None => {
                        output_closed = true;
                        entry.write().await.output_closed = true;
                    } // keep wait/cancel in the OUTER select
                }
            }
        }
    }
    // Stop producers first. Drain only the bounded queue, never wait on a descendant.
    for worker in &workers {
        worker.abort();
    }
    for worker in workers {
        let _ = worker.await;
    }
    while let Ok(text) = output_rx.try_recv() {
        record_output(&entry, &mut log, &text).await;
    }
    stdin_worker.abort();
    let _ = stdin_worker.await;
    let mut guard = entry.write().await;
    guard.stdin_tx = None;
    if let Some(error) = guard.monitor_error.clone() {
        guard.push_output(&format!("\n[{error}]\n"));
    }
    guard.info.pid = None;
    guard.info.completed_at = Some(chrono::Utc::now().timestamp_millis());
    guard.info.exit_code = final_exit_code;
    guard.info.status = if guard.monitor_error.is_some() {
        "failed"
    } else if killed {
        "killed"
    } else if final_exit_code == Some(0) {
        "completed"
    } else {
        "failed"
    }
    .into();
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            log_dir: get_tasks_dir(),
            shutting_down: AtomicBool::new(false),
        }
    }

    pub async fn spawn_task(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        tool_summary: Option<String>,
        is_daemon: bool,
    ) -> Result<TaskInfo, String> {
        let mut cmd = if cfg!(windows) {
            let mut cmd = super::create_hidden_command("powershell.exe");
            cmd.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                command,
            ]);
            cmd
        } else {
            let mut cmd = super::create_hidden_command("sh");
            cmd.args(["-c", command]);
            cmd
        };
        super::git::configure_git_environment(&mut cmd);
        cmd.current_dir(if cwd.trim().is_empty() {
            std::env::temp_dir()
        } else {
            PathBuf::from(cwd)
        });
        self.start_task(session_id, cmd, command, tool_summary, is_daemon)
            .await
    }

    async fn start_task(
        &self,
        session_id: &str,
        mut cmd: Command,
        command: &str,
        tool_summary: Option<String>,
        is_daemon: bool,
    ) -> Result<TaskInfo, String> {
        if session_id.trim().is_empty() {
            return Err("Task session id must not be empty".into());
        }
        // Serialize admission with exit: no child may appear after shutdown's snapshot.
        let mut tasks = self.tasks.write().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("Task manager is shutting down".into());
        }
        let task_id = format!("task_{}", uuid::Uuid::new_v4());
        // Validate/open exclusively BEFORE spawning; never advertise an unavailable log.
        let (path, file) = create_log(&self.log_dir, &task_id).await?;
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .kill_on_drop(false);
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                drop(file);
                let cleanup = tokio::fs::remove_file(&path).await;
                return Err(format!(
                    "Failed to spawn background task: {error}{}",
                    cleanup
                        .err()
                        .map(|e| format!("; failed to remove unused log: {e}"))
                        .unwrap_or_default()
                ));
            }
        };
        let cancel = CancellationToken::new(); // intentionally NOT tied to parent run
        let info = TaskInfo {
            id: task_id.clone(),
            session_id: session_id.into(),
            command: command.into(),
            tool_summary,
            is_daemon,
            status: "running".into(),
            pid: child.id(),
            started_at: chrono::Utc::now().timestamp_millis(),
            completed_at: None,
            exit_code: None,
            log_path: path.to_string_lossy().into_owned(),
            recent_output: String::new(),
        };
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(32);
        let stdin = child.stdin.take();
        let entry = Arc::new(RwLock::new(TaskEntry {
            info: info.clone(),
            output_lines: VecDeque::new(),
            stdin_tx: stdin.as_ref().map(|_| stdin_tx),
            cancel: cancel.clone(),
            monitor_error: None,
            output_closed: false,
        }));
        tasks.insert(task_id, entry.clone());
        let stdin_entry = entry.clone();
        let writer = tokio::spawn(async move {
            if let Some(mut stdin) = stdin {
                loop {
                    let result = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => break,
                        result = async {
                            let Some(msg) = stdin_rx.recv().await else { return Ok(false); };
                            stdin.write_all(msg.as_bytes()).await?;
                            if !msg.ends_with('\n') { stdin.write_all(b"\n").await?; }
                            stdin.flush().await?;
                            Ok::<_, std::io::Error>(true)
                        } => result,
                    };
                    match result {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(error) => {
                            let mut guard = stdin_entry.write().await;
                            guard.push_output(&format!("\n[task stdin write failed: {error}]\n"));
                            guard.stdin_tx = None;
                            break;
                        }
                    }
                }
            }
        });
        tokio::spawn(async move {
            monitor_task(child, entry, file, writer).await;
        });
        Ok(info)
    }

    /// Global readiness check, including daemon tasks and tasks owned by other
    /// sessions. Call while holding the runner's update permit to prevent spawns.
    pub async fn has_running_tasks(&self) -> bool {
        let entries: Vec<_> = self.tasks.read().await.values().cloned().collect();
        for entry in entries {
            if entry.read().await.info.status == "running" {
                return true;
            }
        }
        false
    }

    pub async fn list_tasks(&self, session_id: &str) -> Vec<TaskInfo> {
        let entries: Vec<_> = self.tasks.read().await.values().cloned().collect();
        let mut list = Vec::new();
        for entry in entries {
            let guard = entry.read().await;
            if guard.info.session_id != session_id {
                continue;
            }
            let mut info = guard.info.clone();
            info.recent_output = guard.output(MAX_OUTPUT_LINES);
            list.push(info);
        }
        list.sort_by_key(|task| task.started_at);
        list
    }

    pub async fn get_status(
        &self,
        session_id: &str,
        task_id: &str,
        tail_lines: Option<usize>,
    ) -> Result<TaskInfo, String> {
        let entry = self.entry(session_id, task_id).await?;
        let guard = entry.read().await;
        let mut info = guard.info.clone();
        info.recent_output = guard.output(tail_lines.unwrap_or(50).clamp(1, MAX_OUTPUT_LINES));
        Ok(info)
    }

    async fn entry(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<Arc<RwLock<TaskEntry>>, String> {
        let missing = || format!("Task '{task_id}' not found");
        let entry = self
            .tasks
            .read()
            .await
            .get(task_id)
            .cloned()
            .ok_or_else(missing)?;
        if entry.read().await.info.session_id != session_id {
            return Err(missing());
        }
        Ok(entry)
    }

    pub async fn send_input(
        &self,
        session_id: &str,
        task_id: &str,
        input: &str,
    ) -> Result<String, String> {
        let entry = self.entry(session_id, task_id).await?;
        if input.len() > MAX_INPUT_BYTES {
            return Err(format!("Task input exceeds {MAX_INPUT_BYTES} bytes"));
        }
        let (tx, cancel) = {
            let guard = entry.read().await;
            if guard.info.status != "running"
                || guard.info.pid.is_none() && guard.info.completed_at.is_some()
            {
                return Err(format!(
                    "Task '{task_id}' is not running (status: {})",
                    guard.info.status
                ));
            }
            (
                guard
                    .stdin_tx
                    .clone()
                    .ok_or_else(|| format!("Task '{task_id}' stdin is closed"))?,
                guard.cancel.clone(),
            )
        }; // release the read guard BEFORE ANY channel wait
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(format!("Task '{task_id}' input cancelled")),
            result = tokio::time::timeout(INPUT_TIMEOUT, tx.send(input.to_owned())) => {
                result.map_err(|_| "Task stdin queue timed out".to_string())?
                    .map_err(|_| "Failed to enqueue input to task stdin".to_string())?;
                Ok(format!("Input queued for task '{task_id}'"))
            }
        }
    }

    pub async fn kill_task(&self, session_id: &str, task_id: &str) -> Result<String, String> {
        let entry = self.entry(session_id, task_id).await?;
        {
            let guard = entry.read().await;
            if guard.info.completed_at.is_some() {
                return Ok(format!("Task '{task_id}' is already {}", guard.info.status));
            }
            guard.cancel.cancel();
        }
        tokio::time::timeout(KILL_TIMEOUT, async {
            loop {
                {
                    let guard = entry.read().await;
                    if guard.cancel.is_cancelled() {
                        if let Some(error) = &guard.monitor_error {
                            return Err(error.clone());
                        }
                    }
                    if guard.info.completed_at.is_some() {
                        if let Some(error) = &guard.monitor_error {
                            return Err(error.clone());
                        }
                        return Ok(format!("Task '{task_id}' terminated"));
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| format!("Task '{task_id}' termination was not confirmed before timeout"))?
    }

    pub async fn kill_session_tasks(&self, session_id: &str) {
        let tasks = self.list_tasks(session_id).await;
        for task in tasks {
            if task.completed_at.is_none() {
                let _ = self.kill_task(session_id, &task.id).await;
            }
        }
    }

    /// Final process exit only: close admission, cancel every session concurrently,
    /// then await monitor-confirmed cleanup under one deadline (not one per task).
    pub async fn shutdown(&self, deadline: Duration) -> Result<(), String> {
        self.shutting_down.store(true, Ordering::Release);
        let mut errors = Vec::new();
        let result = tokio::time::timeout(deadline, async {
            let entries: Vec<_> = self.tasks.read().await.values().cloned().collect();
            let mut pending: futures_util::stream::FuturesUnordered<_> = entries
                .into_iter()
                .map(|entry| async move {
                    entry.read().await.cancel.cancel();
                    loop {
                        {
                            let guard = entry.read().await;
                            if guard.info.completed_at.is_some() {
                                return guard
                                    .monitor_error
                                    .as_ref()
                                    .map(|error| format!("Task '{}': {error}", guard.info.id));
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .collect();
            use futures_util::StreamExt;
            while let Some(error) = pending.next().await {
                if let Some(error) = error {
                    errors.push(error);
                }
            }
        })
        .await;
        if result.is_err() {
            // Never fabricate completion or abort a monitor still owning a live PID.
            errors.push("Task shutdown timed out; process cleanup was not confirmed".into());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exit_shutdown_waits_for_busy_monitor_cleanup() {
        let mgr = TaskManager::new();
        let info = mgr
            .start_task(
                "test-session",
                fixture_command("sleep"),
                "exit cleanup fixture",
                None,
                true,
            )
            .await
            .unwrap();
        let entry = mgr.entry("test-session", &info.id).await.unwrap();
        let busy = entry.write().await;
        let shutdown = mgr.shutdown(KILL_TIMEOUT);
        let (result, ()) = tokio::join!(shutdown, async {
            tokio::time::sleep(Duration::from_millis(25)).await;
            drop(busy);
        });
        result.unwrap();
        let done = mgr
            .get_status("test-session", &info.id, None)
            .await
            .unwrap();
        // Clean up the owned fixture even when the regression is RED.
        entry.read().await.cancel.cancel();
        finished(&mgr, "test-session", &info.id).await;
        assert!(
            done.completed_at.is_some(),
            "exit shutdown returned before the busy monitor confirmed process cleanup"
        );
        assert!(done.pid.is_none());
        assert_eq!(done.status, "killed");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn exit_shutdown_terminates_live_process_tree() {
        let mgr = TaskManager::new();
        let info = mgr
            .start_task(
                "tree",
                fixture_command("live-tree"),
                "live tree",
                None,
                true,
            )
            .await
            .unwrap();
        let pid = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let status = mgr.get_status("tree", &info.id, None).await.unwrap();
                if let Some(pid) = status
                    .recent_output
                    .lines()
                    .find_map(|line| line.strip_prefix("descendant-pid=")?.parse::<u32>().ok())
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        // Hold the live descendant's handle across shutdown to avoid PID reuse.
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, millis: u32) -> u32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        let handle = pid
            .ok()
            .map(|pid| unsafe { OpenProcess(0x00100000, 0, pid) });
        let result = mgr.shutdown(KILL_TIMEOUT).await;
        let handle = handle.expect("fixture must start a live descendant");
        assert!(!handle.is_null(), "must open the live descendant");
        let wait = unsafe {
            let wait = WaitForSingleObject(handle, 0);
            CloseHandle(handle);
            wait
        };
        result.unwrap();
        assert_eq!(wait, 0, "descendant remains alive after shutdown");
        assert!(mgr
            .get_status("tree", &info.id, None)
            .await
            .unwrap()
            .completed_at
            .is_some());
    }

    #[tokio::test]
    async fn exit_shutdown_timeout_preserves_unconfirmed_state_and_closes_admission() {
        let mgr = TaskManager::new();
        let info = mgr
            .start_task(
                "test-session",
                fixture_command("sleep"),
                "timeout",
                None,
                true,
            )
            .await
            .unwrap();
        let entry = mgr.entry("test-session", &info.id).await.unwrap();
        let busy = entry.write().await;
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            mgr.shutdown(Duration::from_millis(25)),
        )
        .await
        .expect("shutdown must bound even lock contention")
        .unwrap_err();
        assert!(error.contains("not confirmed"));
        assert_eq!(busy.info.status, "running");
        assert!(busy.info.completed_at.is_none());
        assert_eq!(busy.info.pid, info.pid);
        busy.cancel.cancel();
        drop(busy);
        finished(&mgr, "test-session", &info.id).await;
        let error = mgr
            .start_task(
                "other-session",
                fixture_command("unique"),
                "late",
                None,
                false,
            )
            .await
            .unwrap_err();
        assert!(error.contains("shutting down"));
        assert_eq!(mgr.list_tasks("other-session").await.len(), 0);
        mgr.shutdown(KILL_TIMEOUT).await.unwrap(); // repeat is safe after late cleanup
    }

    #[tokio::test]
    async fn exit_shutdown_cancels_other_sessions_despite_a_busy_entry_and_preserves_errors() {
        let mgr = TaskManager::new();
        let mut entries = Vec::new();
        for (session, daemon) in [("first", true), ("second", false)] {
            let info = mgr
                .start_task(session, fixture_command("sleep"), "exit", None, daemon)
                .await
                .unwrap();
            entries.push(mgr.entry(session, &info.id).await.unwrap());
        }
        let mut busy = entries[0].write().await;
        busy.monitor_error = Some("injected log failure".into());
        let (result, cancelled) = tokio::join!(mgr.shutdown(KILL_TIMEOUT), async {
            let cancelled = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if entries[1].read().await.cancel.is_cancelled() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .is_ok();
            drop(busy);
            cancelled
        });
        assert!(
            cancelled,
            "one busy entry must not prevent other sessions cancelling"
        );
        assert!(result.unwrap_err().contains("injected log failure"));
        for entry in &entries {
            let guard = entry.read().await;
            assert!(guard.info.completed_at.is_some());
            assert!(guard.info.pid.is_none());
        }
        let guard = entries[0].read().await;
        assert_eq!(guard.info.status, "failed");
        assert!(guard.output(500).contains("injected log failure"));
    }

    #[tokio::test]
    async fn kill_session_tasks_terminates_only_tasks_for_matching_session() {
        let manager = TaskManager::new();
        let target_session = "target-session";
        let other_session = "other-session";

        for (sid, tid) in [(target_session, "t1"), (other_session, "t2")] {
            manager.tasks.write().await.insert(
                tid.to_string(),
                Arc::new(RwLock::new(TaskEntry {
                    info: TaskInfo {
                        id: tid.to_string(),
                        session_id: sid.to_string(),
                        command: "test".into(),
                        tool_summary: None,
                        is_daemon: false,
                        status: "running".into(),
                        pid: None,
                        started_at: 0,
                        completed_at: None,
                        exit_code: None,
                        log_path: String::new(),
                        recent_output: String::new(),
                    },
                    cancel: CancellationToken::new(),
                    output_lines: VecDeque::new(),
                    stdin_tx: None,
                    monitor_error: None,
                    output_closed: false,
                })),
            );
        }

        manager.kill_session_tasks(target_session).await;

        let t1 = manager.tasks.read().await.get("t1").unwrap().clone();
        assert!(t1.read().await.cancel.is_cancelled());

        let t2 = manager.tasks.read().await.get("t2").unwrap().clone();
        assert!(!t2.read().await.cancel.is_cancelled());
    }

    #[tokio::test]
    async fn update_gate_running_tasks_scans_all_sessions_and_task_kinds() {
        let manager = TaskManager::new();
        assert!(!manager.has_running_tasks().await);
        for (index, status) in ["completed", "failed", "killed"].into_iter().enumerate() {
            let id = format!("terminal-{index}");
            manager.tasks.write().await.insert(
                id.clone(),
                Arc::new(RwLock::new(TaskEntry {
                    info: TaskInfo {
                        id,
                        session_id: format!("session-{index}"),
                        command: String::new(),
                        tool_summary: None,
                        is_daemon: index % 2 == 0,
                        status: status.into(),
                        pid: None,
                        started_at: 0,
                        completed_at: Some(1),
                        exit_code: None,
                        log_path: String::new(),
                        recent_output: String::new(),
                    },
                    output_lines: VecDeque::new(),
                    stdin_tx: None,
                    cancel: CancellationToken::new(),
                    monitor_error: None,
                    output_closed: true,
                })),
            );
        }
        assert!(!manager.has_running_tasks().await);
        let entry = manager.tasks.read().await["terminal-2"].clone();
        for daemon in [false, true] {
            {
                let mut guard = entry.write().await;
                guard.info.status = "running".into();
                guard.info.completed_at = None;
                guard.info.is_daemon = daemon;
            }
            assert!(manager.has_running_tasks().await, "daemon={daemon}");
            entry.read().await.cancel.cancel();
            assert!(
                manager.has_running_tasks().await,
                "cancellation must not imply confirmed task completion"
            );
            entry.write().await.info.status = "killed".into();
            assert!(!manager.has_running_tasks().await);
        }
        assert!(!manager.log_dir.exists(), "readiness must not create logs");
    }

    #[tokio::test]
    async fn foreign_task_id_is_indistinguishable_from_missing() {
        let mgr = TaskManager::new();
        let info = mgr
            .start_task(
                "other-session",
                fixture_command("sleep"),
                "private command",
                None,
                true,
            )
            .await
            .unwrap();
        let status = mgr.get_status("test-session", &info.id, None).await;
        let input = mgr
            .send_input("test-session", &info.id, "must not reach stdin")
            .await;
        let kill = mgr.kill_task("test-session", &info.id).await;
        let hidden = |result: Result<String, String>| {
            assert_eq!(result.unwrap_err(), format!("Task '{}' not found", info.id));
        };
        // Ensure RED also cleans up its owned child before asserting.
        mgr.tasks
            .read()
            .await
            .get(&info.id)
            .unwrap()
            .read()
            .await
            .cancel
            .cancel();
        hidden(status.map(|_| "status leaked".into()));
        hidden(input);
        hidden(kill);
    }

    #[tokio::test]
    async fn empty_owner_is_rejected_before_spawn_or_log_creation() {
        let mgr = TaskManager::new();
        for owner in ["", " \t\n"] {
            let result = mgr
                .spawn_task(owner, "echo must-not-spawn", "", None, true)
                .await;
            if let Ok(info) = &result {
                mgr.tasks
                    .read()
                    .await
                    .get(&info.id)
                    .unwrap()
                    .read()
                    .await
                    .cancel
                    .cancel();
            }
            assert_eq!(result.unwrap_err(), "Task session id must not be empty");
        }
        assert!(mgr.tasks.read().await.is_empty());
        assert!(
            !mgr.log_dir.exists(),
            "owner validation must precede log creation"
        );
    }

    #[tokio::test]
    async fn list_tasks_is_scoped_to_owner() {
        let mgr = TaskManager::new();
        let mine = mgr
            .start_task(
                "test-session",
                fixture_command("unique"),
                "mine",
                None,
                true,
            )
            .await
            .unwrap();
        let foreign = mgr
            .start_task(
                "other-session",
                fixture_command("unique"),
                "foreign",
                None,
                true,
            )
            .await
            .unwrap();
        finished(&mgr, "test-session", &mine.id).await;
        finished(&mgr, "other-session", &foreign.id).await;
        let tasks = mgr.list_tasks("test-session").await;
        assert_eq!(tasks.len(), 1, "foreign tasks must not be listed");
        assert_eq!(tasks[0].id, mine.id);
    }

    #[tokio::test]
    async fn task_info_serializes_owning_session() {
        let mgr = TaskManager::new();
        let info = mgr
            .start_task(
                "test-session",
                fixture_command("unique"),
                "session fixture",
                None,
                true,
            )
            .await
            .unwrap();
        let done = finished(&mgr, "test-session", &info.id).await;
        assert_eq!(
            serde_json::to_value(done).unwrap()["sessionId"],
            "test-session"
        );
    }

    #[tokio::test]
    async fn test_spawn_and_complete_task() {
        let mgr = TaskManager::new();
        let cmd = if cfg!(windows) {
            "Write-Output 'task completed successfully'"
        } else {
            "echo 'task completed successfully'"
        };

        let info = mgr
            .spawn_task("test-session", cmd, "", Some("Test task".into()), false)
            .await
            .expect("spawn task");

        assert_eq!(info.status, "running");
        assert!(info.id.starts_with("task_"));

        // Wait for completion (give enough headroom under parallel test load)
        let mut completed = false;
        for _ in 0..450 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let status = mgr
                .get_status("test-session", &info.id, Some(10))
                .await
                .expect("get status");
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
            .spawn_task("test-session", cmd, "", Some("Sleep task".into()), true)
            .await
            .expect("spawn sleep task");

        assert_eq!(info.status, "running");
        assert!(info.is_daemon);

        // Kill the task
        let kill_res = mgr
            .kill_task("test-session", &info.id)
            .await
            .expect("kill task");
        assert!(kill_res.contains("terminated"));

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let status = mgr
            .get_status("test-session", &info.id, None)
            .await
            .expect("status after kill");
        assert_eq!(status.status, "killed");
    }

    async fn finished(mgr: &TaskManager, session_id: &str, id: &str) -> TaskInfo {
        tokio::time::timeout(std::time::Duration::from_secs(45), async {
            loop {
                let info = mgr.get_status(session_id, id, Some(500)).await.unwrap();
                if info.completed_at.is_some() {
                    return info;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("monitor must finish")
    }

    fn fixture_command(mode: &str) -> Command {
        let mut command =
            super::super::create_hidden_command(std::env::current_exe().unwrap().to_str().unwrap());
        command.env("AETHER_TASK_FIXTURE", mode).args([
            "--ignored",
            "--exact",
            "agent::tasks::tests::task_fixture",
            "--nocapture",
        ]);
        command
    }

    #[test]
    #[ignore = "child-process fixture, invoked explicitly by regression tests"]
    fn task_fixture() {
        use std::io::Write;
        match std::env::var("AETHER_TASK_FIXTURE").unwrap().as_str() {
            "flood" => {
                let mut out = std::io::stdout().lock();
                out.write_all(&vec![b'x'; 2 * 1024 * 1024]).unwrap();
                out.write_all(b"\n").unwrap();
                out.flush().unwrap();
            }
            "unique" => println!("unique"),
            "sleep" => std::thread::sleep(Duration::from_secs(30)),
            "live-tree" => {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .env("AETHER_TASK_FIXTURE", "sleep")
                    .args([
                        "--ignored",
                        "--exact",
                        "agent::tasks::tests::task_fixture",
                        "--nocapture",
                    ])
                    .spawn()
                    .unwrap();
                println!("descendant-pid={}", child.id());
                std::io::stdout().flush().unwrap();
                std::thread::sleep(Duration::from_secs(30));
                drop(child); // self-expiring fixture if a regression leaves it alive
            }
            "pipeholder" => std::thread::sleep(Duration::from_secs(4)),
            "descendant" => {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .env("AETHER_TASK_FIXTURE", "pipeholder")
                    .args([
                        "--ignored",
                        "--exact",
                        "agent::tasks::tests::task_fixture",
                        "--nocapture",
                    ])
                    .spawn()
                    .unwrap();
                // Deliberately detached inherited-pipe fixture, self-expires in 4s.
                drop(child);
            }
            "eof" => {
                // Close the actual pipe handles, not just a buffered wrapper.
                #[cfg(windows)]
                unsafe {
                    #[link(name = "kernel32")]
                    extern "system" {
                        fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
                        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
                    }
                    CloseHandle(GetStdHandle(-11_i32 as u32));
                    CloseHandle(GetStdHandle(-12_i32 as u32));
                }
                #[cfg(unix)]
                unsafe {
                    extern "C" {
                        fn close(fd: i32) -> i32;
                    }
                    close(1);
                    close(2);
                }
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
            _ => panic!("unknown fixture"),
        }
    }

    #[tokio::test]
    async fn saturated_stdin_does_not_hold_entry_lock() {
        let mgr = TaskManager::new();
        let (tx, rx) = mpsc::channel(1);
        tx.send("full".into()).await.unwrap();
        let entry = Arc::new(RwLock::new(TaskEntry {
            info: TaskInfo {
                id: "task_saturated".into(),
                session_id: "test-session".into(),
                command: String::new(),
                tool_summary: None,
                is_daemon: true,
                status: "running".into(),
                pid: None,
                started_at: 0,
                completed_at: None,
                exit_code: None,
                log_path: String::new(),
                recent_output: String::new(),
            },
            output_lines: VecDeque::new(),
            stdin_tx: Some(tx),
            cancel: CancellationToken::new(),
            monitor_error: None,
            output_closed: false,
        }));
        mgr.tasks
            .write()
            .await
            .insert("task_saturated".into(), entry.clone());
        let send = mgr.send_input("test-session", "task_saturated", "blocked");
        tokio::pin!(send);
        tokio::select! {
            _ = &mut send => panic!("queue should be full"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {}
        }
        let lock = tokio::time::timeout(std::time::Duration::from_millis(250), entry.write()).await;
        let writable = lock.is_ok();
        if let Ok(guard) = lock {
            guard.cancel.cancel();
        }
        drop(rx); // release even on RED; no hung test future
        let result = send.await;
        assert!(
            writable,
            "stdin backpressure must not retain the entry read guard"
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn task_ids_do_not_repeat_between_managers() {
        let a = TaskManager::new();
        let mut b = TaskManager::new();
        b.log_dir = a.log_dir.clone();
        let cmd = if cfg!(windows) {
            "Write-Output unique"
        } else {
            "echo unique"
        };
        let first = a
            .start_task("test-session", fixture_command("unique"), cmd, None, false)
            .await
            .unwrap();
        let second = b
            .start_task("test-session", fixture_command("unique"), cmd, None, false)
            .await
            .unwrap();
        finished(&a, "test-session", &first.id).await;
        finished(&b, "test-session", &second.id).await;
        assert_ne!(
            first.id, second.id,
            "restart must not reuse task identities"
        );
        assert!(uuid::Uuid::parse_str(first.id.strip_prefix("task_").unwrap()).is_ok());
        assert_ne!(first.log_path, second.log_path);
    }

    #[tokio::test]
    async fn newline_free_output_and_log_have_byte_quotas() {
        let mgr = TaskManager::new();
        let info = mgr
            .start_task(
                "test-session",
                fixture_command("flood"),
                "fixture flood",
                None,
                false,
            )
            .await
            .unwrap();
        let done = finished(&mgr, "test-session", &info.id).await;
        assert_eq!(done.status, "completed");
        let listed = mgr.list_tasks("test-session").await;
        assert!(
            listed[0].recent_output.len() <= 64 * 1024,
            "retained output exceeds byte quota"
        );
        assert!(
            std::fs::metadata(&info.log_path).unwrap().len() <= 1024 * 1024,
            "log exceeds quota"
        );
        assert!(tokio::fs::read_to_string(&info.log_path)
            .await
            .unwrap()
            .contains(LOG_TRUNCATED));
        assert!(!listed[0].recent_output.is_empty());
    }

    #[tokio::test]
    async fn eof_before_exit_remains_cancellable() {
        let mgr = TaskManager::new();
        // Direct child: no shell can retain pipes after the fixture closes them.
        let info = mgr
            .start_task(
                "test-session",
                fixture_command("eof"),
                "fixture eof",
                None,
                true,
            )
            .await
            .unwrap();
        let entry = mgr.tasks.read().await.get(&info.id).unwrap().clone();
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if entry.read().await.output_closed {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("fixture must close both output pipes before cancellation");
        entry.read().await.cancel.cancel();
        let done = tokio::time::timeout(KILL_TIMEOUT, finished(&mgr, "test-session", &info.id))
            .await
            .unwrap();
        assert!(done.completed_at.is_some());
        assert!(
            done.pid.is_none(),
            "completed process identity must be retired"
        );
    }

    #[tokio::test]
    async fn test_task_not_found() {
        let mgr = TaskManager::new();
        let err = mgr
            .get_status("test-session", "nonexistent_task", None)
            .await
            .unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn split_utf8_and_invalid_bytes_are_not_silently_lost() {
        let mut pending = Vec::new();
        assert_eq!(decode_chunk(&mut pending, &[0xe2], false), "");
        assert_eq!(
            decode_chunk(&mut pending, &[0x82, 0xac, 0xff], false),
            "€\u{fffd}"
        );
        assert_eq!(decode_chunk(&mut pending, &[0xe2], false), "");
        assert_eq!(decode_chunk(&mut pending, &[], true), "\u{fffd}");
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn log_collision_and_unavailable_directory_are_explicit_errors() {
        let mgr = TaskManager::new();
        let (path, file) = create_log(&mgr.log_dir, "task_existing").await.unwrap();
        drop(file);
        tokio::fs::write(&path, b"original").await.unwrap();
        assert!(create_log(&mgr.log_dir, "task_existing").await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"original");
        let blocked = TaskManager {
            tasks: RwLock::new(HashMap::new()),
            log_dir: path,
            shutting_down: AtomicBool::new(false),
        };
        let error = blocked
            .start_task(
                "test-session",
                fixture_command("unique"),
                "must not spawn",
                None,
                false,
            )
            .await
            .unwrap_err();
        assert!(error.contains("log directory"));
        assert!(blocked.list_tasks("test-session").await.is_empty());
    }

    #[tokio::test]
    async fn quota_marker_and_write_failures_are_explicit() {
        let mut log = TaskLog {
            writer: Vec::<u8>::new(),
            bytes: 0,
            truncated: false,
        };
        for _ in 0..300 {
            log.write(&"界".repeat(4096)).await.unwrap();
        }
        assert!(log.bytes <= MAX_LOG_BYTES);
        assert!(String::from_utf8(log.writer)
            .unwrap()
            .ends_with(LOG_TRUNCATED));

        struct Broken;
        impl AsyncWrite for Broken {
            fn poll_write(
                self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
                _: &[u8],
            ) -> std::task::Poll<std::io::Result<usize>> {
                std::task::Poll::Ready(Err(std::io::Error::other("injected disk failure")))
            }
            fn poll_flush(
                self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                std::task::Poll::Ready(Ok(()))
            }
            fn poll_shutdown(
                self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                std::task::Poll::Ready(Ok(()))
            }
        }
        let mut broken = TaskLog {
            writer: Broken,
            bytes: 0,
            truncated: false,
        };
        assert!(broken
            .write("data")
            .await
            .unwrap_err()
            .contains("injected disk failure"));
    }

    #[tokio::test]
    async fn real_stdin_backpressure_is_bounded_and_kill_completes() {
        let mgr = TaskManager::new();
        let info = mgr
            .start_task(
                "test-session",
                fixture_command("sleep"),
                "no stdin reader",
                None,
                true,
            )
            .await
            .unwrap();
        let input = "x".repeat(MAX_INPUT_BYTES);
        let mut blocked = false;
        for _ in 0..40 {
            if mgr
                .send_input("test-session", &info.id, &input)
                .await
                .is_err()
            {
                blocked = true;
                break;
            }
        }
        assert!(
            blocked,
            "full pipe/channel must time out, not grow unbounded"
        );
        assert!(mgr
            .send_input("test-session", &info.id, &"x".repeat(MAX_INPUT_BYTES + 1))
            .await
            .is_err());
        let sending = mgr.send_input("test-session", &info.id, &input);
        let killing = mgr.kill_task("test-session", &info.id);
        let (send_result, kill_result) = tokio::join!(sending, killing);
        assert!(send_result.is_err());
        // Some Windows hosts cannot start taskkill before its deadline. That MUST
        // be an explicit failure, never a fabricated termination confirmation.
        if let Err(error) = kill_result {
            assert!(error.contains("termination"));
        }
        let done = finished(&mgr, "test-session", &info.id).await;
        assert!(done.pid.is_none());
        assert!(done.completed_at.is_some());
    }

    #[tokio::test]
    async fn daemon_survives_manager_scope_until_explicit_kill() {
        let entry = {
            let mgr = TaskManager::new();
            let info = mgr
                .start_task(
                    "test-session",
                    fixture_command("sleep"),
                    "detached daemon",
                    None,
                    true,
                )
                .await
                .unwrap();
            mgr.entry("test-session", &info.id).await.unwrap()
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(entry.read().await.info.status, "running");
        entry.read().await.cancel.cancel();
        tokio::time::timeout(KILL_TIMEOUT, async {
            loop {
                if entry.read().await.info.completed_at.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn exited_parent_with_inherited_pipes_has_bounded_drain() {
        let mgr = TaskManager::new();
        let info = mgr
            .start_task(
                "test-session",
                fixture_command("descendant"),
                "inherited pipes",
                None,
                false,
            )
            .await
            .unwrap();
        let done = finished(&mgr, "test-session", &info.id).await;
        assert_eq!(done.status, "completed");
        assert!(done.pid.is_none());
        assert!(done.recent_output.contains("output drain deadline reached"));
    }
}
