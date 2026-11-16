use super::types::{Artifact, FileDiffStat};
use base64::Engine;
use futures_util::StreamExt;
use globset::Glob;
use ignore::WalkBuilder;
use regex::{Regex, RegexBuilder};
use similar::TextDiff;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TOOL_CHARS: usize = 200_000;
const MAX_COMMAND_BYTES: usize = 1024 * 1024;
const MAX_WEB_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum ToolEvent {
    Artifact(Artifact),
    TerminalOutput {
        artifact_id: String,
        delta: String,
        stream: String,
    },
}

pub type ToolEventSink = Arc<dyn Fn(ToolEvent) + Send + Sync>;

#[derive(Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub cwd: String,
    pub cancel: CancellationToken,
    pub sink: Option<ToolEventSink>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolExecutionResult {
    pub output: String,
    pub artifact: Option<Artifact>,
    pub success: bool,
    pub cancelled: bool,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub terminal_output: Option<String>,
    pub files_changed: Option<Vec<FileDiffStat>>,
}

impl ToolExecutionResult {
    pub fn success(output: String, artifact: Option<Artifact>) -> Self {
        Self {
            output,
            artifact,
            success: true,
            cancelled: false,
            ..Default::default()
        }
    }
    pub fn failure(output: String) -> Self {
        Self {
            output,
            artifact: None,
            success: false,
            cancelled: false,
            ..Default::default()
        }
    }
}

/// Shared review/foreground skill contracts. Review fallback schemas reuse these.
pub fn review_tool_definitions() -> serde_json::Value {
    serde_json::json!([
        {"type":"function","function":{"name":"skills_list","description":"List discovered skills for review.","parameters":{"type":"object","properties":{}}}},
        {"type":"function","function":{"name":"skill_view","description":"Read an exact skill file before changing it.","parameters":{"type":"object","properties":{"name":{"type":"string"},"file_path":{"type":"string"}},"required":["name"]}}},
        {"type":"function","function":{"name":"skill_manage","description":"Create or update skills through the restricted review store; preserve metadata and read existing files first.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["create","patch","edit","write_file","remove_file"]},"name":{"type":"string"},"description":{"type":"string"},"content":{"type":"string"},"file_path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"}},"required":["action","name"]}}}
    ])
}

pub fn get_agent_tools() -> serde_json::Value {
    let mut tools = serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a UTF-8 text file in the active workspace. Long reads are opened in the artifact sidecar.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "startLine": {"type": "integer", "minimum": 1},
                        "endLine": {"type": "integer", "minimum": 1}
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Create or atomically overwrite a workspace text file, creating parent directories automatically.",
                "parameters": {
                    "type": "object",
                    "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
                    "required": ["path", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Replace an exact string and return a unified diff. A non-global edit must match exactly once.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}, "oldString": {"type": "string"},
                        "newString": {"type": "string"}, "replaceAll": {"type": "boolean", "default": false}
                    },
                    "required": ["path", "oldString", "newString"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "List a directory or bounded recursive tree in the active workspace.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "default": "."},
                        "recursive": {"type": "boolean", "default": false},
                        "maxDepth": {"type": "integer", "minimum": 1, "maximum": 8, "default": 3},
                        "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 500}
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Search workspace file contents with a regular expression and optional glob.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string"}, "path": {"type": "string", "default": "."},
                        "fileGlob": {"type": "string"}, "caseSensitive": {"type": "boolean", "default": false},
                        "maxResults": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}
                    },
                    "required": ["pattern"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Execute PowerShell asynchronously with streaming terminal artifacts, timeout, and cancellation. For long-running commands, dev servers, or background watchers, set isDaemon: true.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"},
                        "timeoutSeconds": {"type": "integer", "minimum": 1, "maximum": 600, "default": 120},
                        "isDaemon": {"type": "boolean", "description": "Set to true for long-running support processes that should run in the background."},
                        "toolSummary": {"type": "string", "description": "Brief 2-5 word label for the background task"}
                    },
                    "required": ["command"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "manage_task",
                "description": "Manage background tasks and processes: 'list' running/recent tasks, check 'status' and recent logs, 'send_input' to stdin, or 'kill' a running task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list", "status", "send_input", "kill"],
                            "description": "Action to perform: 'list', 'status', 'send_input', or 'kill'"
                        },
                        "taskId": {
                            "type": "string",
                            "description": "The task ID (e.g. 'task_1') as returned by list or run_command with isDaemon: true. Required for status, send_input, and kill."
                        },
                        "input": {
                            "type": "string",
                            "description": "Text input to send to stdin of the task. Required for send_input."
                        },
                        "tailLines": {
                            "type": "integer",
                            "description": "Number of recent log lines to return for status action (default 50, max 500).",
                            "default": 50
                        }
                    },
                    "required": ["action"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "fetch_web_page",
                "description": "Fetch a public HTTP(S) URL and extract bounded Markdown/plain text. Private targets are rejected.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "format": "uri"},
                        "maxChars": {"type": "integer", "minimum": 2000, "maximum": 500000, "default": 100000}
                    },
                    "required": ["url"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "search_web",
                "description": "Performs a web search for a given query keywords. Returns title, snippet, and target URLs of the most relevant search results.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query keywords"
                        },
                        "domain": {
                            "type": "string",
                            "description": "Optional domain to restrict results to, e.g. 'github.com' or 'docs.rs'"
                        },
                        "fetch_content": {
                            "type": "boolean",
                            "description": "When true, automatically fetches and includes concise full-text content extracts from the top search results to save roundtrips"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (1-10, default: 5)"
                        }
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "use_skill",
                "description": "Load and activate a specific skill's instructions from the catalog by its name. Use this whenever the user request matches a known skill capability.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "The exact name of the skill to activate (e.g. 'systematic-debugging', 'codebase-inspection', 'test-driven-development')"
                        }
                    },
                    "required": ["name"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "spawn_subagent",
                "description": "Spawn an autonomous subagent in the background to handle an independent research, debugging, verification, or exploration sub-task. The subagent runs its own tool loop and returns a synthesized final report.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "role": {
                            "type": "string",
                            "description": "A 2-4 word specialized role description (e.g. 'Bug Researcher', 'Codebase Explorer', 'Test Auditor')"
                        },
                        "task": {
                            "type": "string",
                            "description": "Clear, actionable task description for the subagent to execute autonomously"
                        },
                        "subpath": {
                            "type": "string",
                            "description": "Optional subfolder in the workspace for the subagent to focus on"
                        },
                        "max_turns": {
                            "type": "integer",
                            "description": "Optional maximum investigation turns (5-100, default: 25). Set higher (e.g. 40-60) for broad codebase audits, deep refactoring, or multi-file research."
                        }
                    },
                    "required": ["role", "task"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "learn_skill",
                "description": "Synthesize and persist a reusable skill (SKILL.md) to ~/.aether-agent/skills/<name>/SKILL.md. Use this when you discover or complete a repeatable solution, complex multi-step workflow, or when the user asks you to remember or learn how to do something.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Unique kebab-case name of the skill (e.g., 'data-cleaning-pipeline', 'pdf-table-extractor')"
                        },
                        "description": {
                            "type": "string",
                            "description": "A concise one or two sentence description of what the skill does and when to trigger it."
                        },
                        "instructions": {
                            "type": "string",
                            "description": "Markdown instructions, steps, scripts, and best practices explaining how to execute the skill."
                        },
                        "overwrite": {
                            "type": "boolean",
                            "description": "Set to true if you explicitly intend to overwrite an existing skill with the same name. Defaults to false."
                        }
                    },
                    "required": ["name", "description", "instructions"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "ask_question",
                "description": "Ask the user structured clarification or choice questions with interactive selectable options. Use this when facing architectural decisions, dependency choices, ambiguous requirements, or whenever user confirmation is needed.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "questions": {
                            "type": "array",
                            "description": "List of questions to present to the user",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "question": {
                                        "type": "string",
                                        "description": "The specific question text"
                                    },
                                    "options": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "List of choices for the user to select from"
                                    },
                                    "is_multi_select": {
                                        "type": "boolean",
                                        "description": "Whether multiple choices can be selected. Defaults to false."
                                    }
                                },
                                "required": ["question", "options"]
                            }
                        }
                    },
                    "required": ["questions"]
                }
            }
        }
    ]);
    tools.as_array_mut().expect("static tool array").extend(
        review_tool_definitions()
            .as_array()
            .expect("static review tools")
            .iter()
            .cloned(),
    );
    tools
}

pub async fn execute_tool(
    name: &str,
    args: &serde_json::Value,
    context: &ToolContext,
) -> ToolExecutionResult {
    if context.cancel.is_cancelled() {
        return ToolExecutionResult {
            output: "Cancelled before tool execution".into(),
            artifact: None,
            success: false,
            cancelled: true,
            duration_ms: None,
            exit_code: None,
            terminal_output: None,
            files_changed: None,
        };
    }
    let result = match name {
        "read_file" => read_file(args, &context.cwd).await,
        "write_file" => write_file(args, &context.cwd).await,
        "edit_file" => edit_file(args, &context.cwd).await,
        "list_directory" => list_directory(args, &context.cwd, &context.cancel).await,
        "search_files" => search_files(args, &context.cwd, &context.cancel).await,
        "run_command" => run_command(args, context).await,
        "manage_task" => manage_task(args, context).await,
        "fetch_web_page" => fetch_web_page(args, context).await,
        "search_web" => search_web(args, context).await,
        "learn_skill" => learn_skill(args, context).await,
        "ask_question" => ask_question(args).await,
        _ => Err(format!("Unknown tool: {name}")),
    };
    result.unwrap_or_else(ToolExecutionResult::failure)
}

fn required_string<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("Missing required string argument: {key}"))
}

fn optional_string<'a>(args: &'a serde_json::Value, camel: &str, snake: &str) -> Option<&'a str> {
    args.get(camel)
        .and_then(|value| value.as_str())
        .or_else(|| args.get(snake).and_then(|value| value.as_str()))
}

fn new_artifact(
    title: String,
    artifact_type: &str,
    language: Option<String>,
    content: String,
    file_path: Option<String>,
) -> Artifact {
    Artifact {
        stream_revision: None,
        id: format!("art-{}", Uuid::new_v4()),
        title,
        artifact_type: artifact_type.into(),
        language,
        content,
        file_path,
        timestamp: chrono::Utc::now().timestamp_millis(),
    }
}

fn bounded_chars(content: &str, limit: usize) -> (String, bool) {
    if content.len() <= limit {
        return (content.to_string(), false);
    }
    let mut end = limit;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    (
        format!("{}\n… [output truncated at {limit} bytes]", &content[..end]),
        true,
    )
}

async fn read_file(args: &serde_json::Value, cwd: &str) -> Result<ToolExecutionResult, String> {
    let requested = required_string(args, "path")?;
    let path = resolve_workspace_path(cwd, requested)?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|error| format!("Cannot stat {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("Not a regular file: {}", path.display()));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "File exceeds the {} MiB read limit",
            MAX_FILE_BYTES / 1024 / 1024
        ));
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    if bytes.contains(&0) {
        return Err("Binary files are not supported by read_file".into());
    }
    let content =
        String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8 text".to_string())?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Ok(ToolExecutionResult::success(
            format!("Read empty file {}", path.display()),
            None,
        ));
    }
    let start = optional_string(args, "startLine", "start_line")
        .and_then(|value| value.parse::<usize>().ok())
        .or_else(|| {
            args.get("startLine")
                .or_else(|| args.get("start_line"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
        })
        .unwrap_or(1);
    let end = optional_string(args, "endLine", "end_line")
        .and_then(|value| value.parse::<usize>().ok())
        .or_else(|| {
            args.get("endLine")
                .or_else(|| args.get("end_line"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
        })
        .unwrap_or(lines.len());
    if start == 0 || end < start {
        return Err("startLine must be >= 1 and endLine must be >= startLine".into());
    }
    if start > lines.len() && !lines.is_empty() {
        return Err(format!(
            "startLine {start} exceeds the file's {} lines",
            lines.len()
        ));
    }
    let selected = if lines.is_empty() {
        String::new()
    } else {
        lines[start - 1..end.min(lines.len())].join("\n")
    };
    let (selected, truncated) = bounded_chars(&selected, MAX_TOOL_CHARS);
    let selected_line_count = if selected.is_empty() {
        0
    } else {
        selected.lines().count()
    };
    let artifact = if selected_line_count > 25 {
        Some(new_artifact(
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("file")
                .to_string(),
            "code",
            path.extension()
                .and_then(|value| value.to_str())
                .map(str::to_string),
            selected.clone(),
            Some(path.to_string_lossy().to_string()),
        ))
    } else {
        None
    };
    let header = format!(
        "Read lines {start}-{} of {} from {}{}",
        end.min(lines.len()),
        lines.len(),
        path.display(),
        if truncated { " (truncated)" } else { "" }
    );
    Ok(ToolExecutionResult::success(
        format!("{header}\n\n{selected}"),
        artifact,
    ))
}

/// Read an existing file as text for snapshot comparison. A missing file is
/// reported as `None`; a symlink, non-regular file, oversized file, or
/// non-UTF-8 content is an error so a write never clobbers unverifiable data.
async fn read_existing_text(path: &Path) -> Result<Option<String>, String> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "Refusing to write through the symbolic link {}",
                    path.display()
                ));
            }
            if !metadata.is_file() {
                return Err(format!("Not a regular file: {}", path.display()));
            }
            if metadata.len() > MAX_FILE_BYTES {
                return Err("Existing file exceeds the 10 MiB limit".into());
            }
            tokio::fs::read_to_string(path)
                .await
                .map_err(|_| "Existing file is not valid UTF-8 text".to_string())
                .map(Some)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Cannot stat {}: {error}", path.display())),
    }
}

fn diff_line_stats(previous: &str, updated: &str) -> (usize, usize) {
    let mut additions = 0;
    let mut deletions = 0;
    for change in TextDiff::from_lines(previous, updated).iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => additions += 1,
            similar::ChangeTag::Delete => deletions += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (additions, deletions)
}

// Shared by every in-process tool/subagent. Weak entries do not retain locks
// forever. Callers must hold the guard from their initial read through write.
async fn lock_file_mutation(path: &Path) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    type Locks = std::collections::HashMap<PathBuf, std::sync::Weak<tokio::sync::Mutex<()>>>;
    static LOCKS: std::sync::OnceLock<std::sync::Mutex<Locks>> = std::sync::OnceLock::new();
    let lock = {
        let mut locks = LOCKS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "File mutation lock was poisoned")?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        // Canonical paths normalize aliases; Windows comparison is case-insensitive.
        #[cfg(windows)]
        let key = PathBuf::from(path.to_string_lossy().to_lowercase());
        #[cfg(not(windows))]
        let key = path.to_path_buf();
        if let Some(lock) = locks.get(&key).and_then(std::sync::Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(tokio::sync::Mutex::new(()));
            locks.insert(key, Arc::downgrade(&lock));
            lock
        }
    };
    Ok(lock.lock_owned().await)
}

/// Recheck the snapshot before atomic replacement. This catches changes already
/// visible at recheck, NOT external-editor writes between recheck and rename.
async fn guarded_atomic_write(
    path: &Path,
    bytes: &[u8],
    expected: Option<&str>,
) -> Result<(), String> {
    let current = read_existing_text(path).await?;
    match (expected, current.as_deref()) {
        (None, None) => {}
        (None, Some(_)) => {
            return Err(format!(
                "{} appeared on disk after the workspace was read; refusing to overwrite it",
                path.display()
            ));
        }
        (Some(expected), Some(current)) if current == expected => {}
        (Some(_), None) => {
            return Err(format!(
                "{} disappeared from disk after the workspace was read; refusing to recreate it",
                path.display()
            ));
        }
        (Some(_), Some(_)) => {
            return Err(format!(
                "{} changed on disk after the workspace was read; re-read it and retry",
                path.display()
            ));
        }
    }
    super::storage::atomic_write(path, bytes).await
}

async fn write_file(args: &serde_json::Value, cwd: &str) -> Result<ToolExecutionResult, String> {
    let path = resolve_workspace_path(cwd, required_string(args, "path")?)?;
    let content = required_string(args, "content")?;
    if content.len() > MAX_FILE_BYTES as usize {
        return Err("write_file content exceeds 10 MiB".into());
    }
    let _mutation = lock_file_mutation(&path).await?;
    let rel_path = workspace_relative_path(cwd, &path)?;
    let previous = read_existing_text(&path).await?;
    let (additions, deletions) = diff_line_stats(previous.as_deref().unwrap_or(""), content);
    guarded_atomic_write(&path, content.as_bytes(), previous.as_deref()).await?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let artifact = Some(new_artifact(
        format!("Saved: {}", filename),
        "code",
        path.extension()
            .and_then(|value| value.to_str())
            .map(str::to_string),
        content.to_string(),
        Some(path.to_string_lossy().to_string()),
    ));
    let mut res = ToolExecutionResult::success(
        format!("Wrote {} bytes to {}", content.len(), path.display()),
        artifact,
    );
    res.files_changed = Some(vec![FileDiffStat {
        path: rel_path,
        additions,
        deletions,
    }]);
    Ok(res)
}

fn checked_edit_result_len(
    original_len: usize,
    matches: usize,
    old_len: usize,
    new_len: usize,
) -> Result<usize, String> {
    let projected = matches.checked_mul(old_len).and_then(|removed| {
        original_len.checked_sub(removed).and_then(|remaining| {
            matches
                .checked_mul(new_len)
                .and_then(|added| remaining.checked_add(added))
        })
    });
    projected
        .filter(|&len| len <= MAX_FILE_BYTES as usize)
        .ok_or_else(|| "edit_file result exceeds the 10 MiB limit".into())
}

#[cfg(test)]
tokio::task_local! {
    static FORBID_EDIT_ALLOCATION: bool;
    static EDIT_DIFF_BUILDS: std::cell::Cell<usize>;
}

async fn edit_file(args: &serde_json::Value, cwd: &str) -> Result<ToolExecutionResult, String> {
    let path = resolve_workspace_path(cwd, required_string(args, "path")?)?;
    let old = optional_string(args, "oldString", "old_string").ok_or("Missing oldString")?;
    let new = optional_string(args, "newString", "new_string").ok_or("Missing newString")?;
    if old.is_empty() {
        return Err("oldString must not be empty".into());
    }
    let replace_all = args
        .get("replaceAll")
        .or_else(|| args.get("replace_all"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let _mutation = lock_file_mutation(&path).await?;
    let original = {
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|error| format!("Cannot stat {}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("Not a regular file: {}", path.display()));
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err("edit_file input exceeds the 10 MiB limit".into());
        }
        tokio::fs::read_to_string(&path)
            .await
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))?
    };
    let matches = original.matches(old).count();
    if matches == 0 {
        return Err("oldString was not found; no changes were written".into());
    }
    if matches > 1 && !replace_all {
        return Err(format!(
            "oldString matched {matches} locations; set replaceAll=true or provide a unique target"
        ));
    }
    checked_edit_result_len(original.len(), matches, old.len(), new.len())?;
    #[cfg(test)]
    assert!(
        !FORBID_EDIT_ALLOCATION.try_with(|forbid| *forbid).unwrap_or(false),
        "edit_file attempted replacement allocation before rejecting oversized output"
    );
    let updated = if replace_all {
        original.replace(old, new)
    } else {
        original.replacen(old, new, 1)
    };
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    #[cfg(test)]
    let _ = EDIT_DIFF_BUILDS.try_with(|builds| builds.set(builds.get() + 1));
    let text_diff = TextDiff::from_lines(&original, &updated);
    let diff = text_diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{filename}"), &format!("b/{filename}"))
        .to_string();
    let mut additions = 0;
    let mut deletions = 0;
    for change in text_diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => additions += 1,
            similar::ChangeTag::Delete => deletions += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    let rel_path = workspace_relative_path(cwd, &path)?;
    guarded_atomic_write(&path, updated.as_bytes(), Some(&original)).await?;
    let artifact = new_artifact(
        format!("Diff: {filename}"),
        "diff",
        Some("diff".into()),
        diff.clone(),
        Some(path.to_string_lossy().to_string()),
    );
    let mut res = ToolExecutionResult::success(
        format!(
            "Updated {} at {} replacement(s)",
            path.display(),
            if replace_all { matches } else { 1 }
        ),
        Some(artifact),
    );
    res.files_changed = Some(vec![FileDiffStat {
        path: rel_path,
        additions,
        deletions,
    }]);
    Ok(res)
}

async fn list_directory(
    args: &serde_json::Value,
    cwd: &str,
    cancel: &CancellationToken,
) -> Result<ToolExecutionResult, String> {
    let path = resolve_workspace_path(
        cwd,
        args.get("path").and_then(|v| v.as_str()).unwrap_or("."),
    )?;
    if !path.is_dir() {
        return Err(format!("Not a directory: {}", path.display()));
    }
    let recursive = args
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_depth = args
        .get("maxDepth")
        .and_then(|v| v.as_u64())
        .unwrap_or(3)
        .clamp(1, 8) as usize;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(500)
        .clamp(1, 1_000) as usize;
    let root = path.clone();
    let worker_cancel = cancel.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<(Vec<String>, bool), String> {
        let mut rows = Vec::new();
        let mut truncated = false;
        for entry in WalkBuilder::new(&path)
            .hidden(false)
            .max_depth(Some(if recursive { max_depth } else { 1 }))
            .build()
            .filter_map(Result::ok)
        {
            if worker_cancel.is_cancelled() {
                return Err("Directory listing cancelled".into());
            }
            if entry.path() == root {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            let depth = relative.split('/').count().saturating_sub(1);
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            rows.push(format!(
                "{}{}{}",
                "  ".repeat(depth),
                if is_dir { "[dir] " } else { "      " },
                relative.split('/').next_back().unwrap_or(&relative)
            ));
            if rows.len() >= limit {
                truncated = true;
                break;
            }
        }
        Ok((rows, truncated))
    })
    .await
    .map_err(|error| error.to_string())?;
    if cancel.is_cancelled() {
        return Ok(ToolExecutionResult {
            output: "Directory listing cancelled".into(),
            success: false,
            cancelled: true,
            ..Default::default()
        });
    }
    let (mut rows, truncated) = result?;
    rows.sort();
    if rows.is_empty() {
        rows.push("(empty directory)".into());
    }
    if truncated {
        rows.push(format!("… [tree truncated at {limit} entries]"));
    }
    let output = rows.join("\n");
    let artifact = if rows.len() > 25 {
        Some(new_artifact(
            "Directory tree".into(),
            "file",
            Some("text".into()),
            output.clone(),
            None,
        ))
    } else {
        None
    };
    Ok(ToolExecutionResult::success(output, artifact))
}

async fn search_files(
    args: &serde_json::Value,
    cwd: &str,
    cancel: &CancellationToken,
) -> Result<ToolExecutionResult, String> {
    let pattern = required_string(args, "pattern")?.to_string();
    let path = resolve_workspace_path(
        cwd,
        args.get("path").and_then(|v| v.as_str()).unwrap_or("."),
    )?;
    let case_sensitive = args
        .get("caseSensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let regex = RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|error| format!("Invalid regex: {error}"))?;
    let glob = match args
        .get("fileGlob")
        .or_else(|| args.get("file_glob"))
        .and_then(|v| v.as_str())
    {
        Some(value) if !value.is_empty() => Some(
            Glob::new(value)
                .map_err(|error| format!("Invalid fileGlob: {error}"))?
                .compile_matcher(),
        ),
        _ => None,
    };
    let limit = args
        .get("maxResults")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .clamp(1, 500) as usize;
    let root = path.clone();
    let worker_cancel = cancel.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<String>, String> {
        let mut matches = Vec::new();
        for entry in WalkBuilder::new(&path)
            .hidden(false)
            .build()
            .filter_map(Result::ok)
        {
            if worker_cancel.is_cancelled() {
                return Err("File search cancelled".into());
            }
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let file_path = entry.path();
            let relative = file_path
                .strip_prefix(&root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .replace('\\', "/");
            if glob
                .as_ref()
                .is_some_and(|matcher| !matcher.is_match(&relative))
            {
                continue;
            }
            if entry
                .metadata()
                .map(|meta| meta.len())
                .unwrap_or(MAX_FILE_BYTES + 1)
                > 2 * 1024 * 1024
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(file_path) else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }
            let Ok(content) = String::from_utf8(bytes) else {
                continue;
            };
            for (index, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    let (line, _) = bounded_chars(line, 2_000);
                    matches.push(format!("{relative}:{}:{line}", index + 1));
                    if matches.len() >= limit {
                        return Ok(matches);
                    }
                }
            }
        }
        Ok(matches)
    })
    .await
    .map_err(|error| error.to_string())?;
    if cancel.is_cancelled() {
        return Ok(ToolExecutionResult {
            output: "File search cancelled".into(),
            success: false,
            cancelled: true,
            ..Default::default()
        });
    }
    let rows = result?;
    let output = if rows.is_empty() {
        "No matches found".into()
    } else {
        rows.join("\n")
    };
    let artifact = if rows.len() > 25 {
        Some(new_artifact(
            "Search results".into(),
            "file",
            Some("text".into()),
            output.clone(),
            None,
        ))
    } else {
        None
    };
    Ok(ToolExecutionResult::success(output, artifact))
}

#[derive(Debug)]
struct ProcessLine {
    stream: String,
    text: String,
}

fn take_utf8_output(buffer: &mut Vec<u8>, eof: bool) -> String {
    if buffer.is_empty() {
        return String::new();
    }
    match std::str::from_utf8(buffer) {
        Ok(_) => String::from_utf8(std::mem::take(buffer)).unwrap_or_default(),
        Err(error) if error.error_len().is_none() && !eof => {
            let valid = error.valid_up_to();
            let remainder = buffer.split_off(valid);
            let text = String::from_utf8(std::mem::take(buffer)).unwrap_or_default();
            *buffer = remainder;
            text
        }
        Err(_) => String::from_utf8_lossy(&std::mem::take(buffer)).into_owned(),
    }
}

fn spawn_process_reader<R>(
    mut reader: R,
    stream: &'static str,
    sender: Sender<ProcessLine>,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut pending_utf8 = Vec::new();
        let mut chunk = [0u8; 8 * 1024];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => {
                    let text = take_utf8_output(&mut pending_utf8, true);
                    if !text.is_empty() {
                        let _ = sender
                            .send(ProcessLine {
                                stream: stream.into(),
                                text,
                            })
                            .await;
                    }
                    break;
                }
                Ok(count) => {
                    pending_utf8.extend_from_slice(&chunk[..count]);
                    let text = take_utf8_output(&mut pending_utf8, false);
                    if !text.is_empty()
                        && sender
                            .send(ProcessLine {
                                stream: stream.into(),
                                text,
                            })
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
#[derive(Default)]
struct ForegroundProbe {
    polls: std::sync::atomic::AtomicUsize,
    closed_while_running: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
tokio::task_local! {
    static FOREGROUND_PROBE: Arc<ForegroundProbe>;
}

async fn run_command(
    args: &serde_json::Value,
    context: &ToolContext,
) -> Result<ToolExecutionResult, String> {
    let command = required_string(args, "command")?.trim();
    if command.is_empty() {
        return Err("Command must not be empty".into());
    }
    if command.len() > 32 * 1024 {
        return Err("Command exceeds 32 KiB".into());
    }
    let timeout_seconds = args
        .get("timeoutSeconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(120)
        .clamp(1, 600);

    let is_daemon = args
        .get("isDaemon")
        .or_else(|| args.get("is_daemon"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tool_summary = optional_string(args, "toolSummary", "tool_summary").map(|s| s.to_string());

    if is_daemon {
        let mgr = super::tasks::get_task_manager();
        let task_info = mgr
            .spawn_task(
                &context.session_id,
                command,
                &context.cwd,
                tool_summary,
                true,
            )
            .await?;
        let output_msg = format!(
            "Background task started successfully.\nTaskId: {}\nStatus: {}\nPID: {}\nLog Path: {}\nUse `manage_task` with action 'status' to monitor output or 'kill' to terminate.",
            task_info.id,
            task_info.status,
            task_info.pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".into()),
            task_info.log_path
        );
        return Ok(ToolExecutionResult::success(output_msg, None));
    }

    let root = if context.cwd.trim().is_empty() {
        std::env::temp_dir()
    } else {
        resolve_workspace_path(&context.cwd, ".")?
    };
    let mut process = if cfg!(windows) {
        let mut process = super::create_hidden_command("powershell.exe");
        process.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command,
        ]);
        process
    } else {
        let mut process = super::create_hidden_command("sh");
        process.args(["-c", command]);
        process
    };
    // Shell-launched git push/fetch inherits non-interactive credentials; the
    // foreground command loop below enforces its 1..600 second deadline.
    super::git::configure_git_environment(&mut process);
    process
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let mut child = process
        .spawn()
        .map_err(|error| format!("Failed to start PowerShell: {error}"))?;
    let stdout = child.stdout.take().ok_or("Could not capture stdout")?;
    let stderr = child.stderr.take().ok_or("Could not capture stderr")?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<ProcessLine>(256);
    let stdout_task = spawn_process_reader(stdout, "stdout", sender.clone());
    let stderr_task = spawn_process_reader(stderr, "stderr", sender.clone());
    drop(sender);
    #[cfg(test)]
    if FOREGROUND_PROBE.try_with(|_| ()).is_ok() {
        // Tutup pembaca agar EOF saat proses masih hidup dapat diuji tanpa balapan shell.
        stdout_task.abort();
        stderr_task.abort();
        while receiver.recv().await.is_some() {}
    }

    let artifact_id = format!("art-{}", Uuid::new_v4());
    let title = format!("Terminal: {}", command.chars().take(80).collect::<String>());
    if let Some(sink) = &context.sink {
        sink(ToolEvent::Artifact(Artifact {
            stream_revision: None,
            id: artifact_id.clone(),
            title: title.clone(),
            artifact_type: "terminal".into(),
            language: Some("powershell".into()),
            content: String::new(),
            file_path: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }));
    }

    let mut output = String::new();
    let mut total_bytes = 0usize;
    let mut output_truncated = false;
    let started = Instant::now();
    let mut exit_code: Option<i32> = None;
    let mut cancelled = false;
    let mut timed_out = false;
    let mut output_closed = false;
    loop {
        #[cfg(test)]
        let _ = FOREGROUND_PROBE.try_with(|probe| {
            probe.polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        if let Ok(Some(status)) = child.try_wait() {
            exit_code = Some(status.code().unwrap_or(-1));
            break;
        }
        if context.cancel.is_cancelled() {
            cancelled = true;
            terminate_child(&mut child).await;
            break;
        }
        let remaining = Duration::from_secs(timeout_seconds).saturating_sub(started.elapsed());
        if remaining.is_zero() {
            timed_out = true;
            terminate_child(&mut child).await;
            break;
        }
        // EOF tetap tertutup; tunggu tanpa memutar sambil menjaga pembatalan dan tenggat.
        tokio::select! {
            _ = context.cancel.cancelled() => {},
            _ = tokio::time::sleep(remaining.min(Duration::from_millis(50))) => {},
            line = receiver.recv(), if !output_closed => match line {
                Some(line) => append_process_line(
                    &mut output,
                    &mut total_bytes,
                    &mut output_truncated,
                    &line,
                    &artifact_id,
                    &context.sink,
                ),
                None => {
                    output_closed = true;
                    #[cfg(test)]
                    if matches!(child.try_wait(), Ok(None)) {
                        let _ = FOREGROUND_PROBE.try_with(|probe| {
                            probe
                                .closed_while_running
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        });
                    }
                }
            },
        }
    }
    if (cancelled || timed_out)
        && tokio::time::timeout(Duration::from_secs(1), child.wait())
            .await
            .is_err()
    {
        // A terminated shell must never hold the agent indefinitely while its
        // status is reaped. kill_on_drop cleans up if even this wait stalls.
        let _ = child.start_kill();
    }
    let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let remaining = drain_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, receiver.recv()).await {
            Ok(Some(line)) => append_process_line(
                &mut output,
                &mut total_bytes,
                &mut output_truncated,
                &line,
                &artifact_id,
                &context.sink,
            ),
            _ => break,
        }
    }
    stdout_task.abort();
    stderr_task.abort();
    if output_truncated {
        output.push_str("\n… [terminal output truncated at 1 MiB]\n");
    }
    if timed_out {
        output.push_str(&format!(
            "\nCommand timed out after {timeout_seconds}s and was terminated.\n"
        ));
    }
    if cancelled {
        output.push_str("\nCommand cancelled by user.\n");
    }
    let artifact = Artifact {
        stream_revision: None,
        id: artifact_id,
        title,
        artifact_type: "terminal".into(),
        language: Some("powershell".into()),
        content: output.clone(),
        file_path: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    };
    let summary = format!(
        "Command finished. Exit code: {}. Output bytes: {}.\n{}",
        exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated".into()),
        total_bytes,
        output
    );
    let duration_ms = started.elapsed().as_millis() as u64;
    let final_exit_code = exit_code.unwrap_or(if !cancelled && !timed_out { 0 } else { 1 });
    Ok(ToolExecutionResult {
        output: summary,
        artifact: Some(artifact),
        success: !cancelled && !timed_out && exit_code == Some(0),
        cancelled,
        duration_ms: Some(duration_ms),
        exit_code: Some(final_exit_code),
        terminal_output: Some(output),
        files_changed: None,
    })
}

async fn learn_skill(
    args: &serde_json::Value,
    _context: &ToolContext,
) -> Result<ToolExecutionResult, String> {
    let name = required_string(args, "name")?;
    let description = required_string(args, "description")?;
    let instructions = required_string(args, "instructions")?;
    let overwrite = args
        .get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let skill =
        super::skills::save_skill_markdown(name, description, instructions, overwrite).await?;
    let artifact = Some(new_artifact(
        format!("Learned Skill: {}", skill.name),
        "markdown",
        Some("markdown".into()),
        format!(
            "# {}\n**Description:** {}\n\n{}",
            skill.name, skill.description, instructions
        ),
        Some(skill.path.clone()),
    ));

    Ok(ToolExecutionResult::success(
        format!(
            "Successfully learned skill '{}' and saved to {}. It is now registered and active in the agent skill registry.",
            skill.name, skill.path
        ),
        artifact,
    ))
}

async fn terminate_child(child: &mut Child) {
    #[cfg(windows)]
    if let Some(id) = child.id() {
        let killer = super::create_hidden_command("taskkill")
            .kill_on_drop(true)
            .arg("/PID")
            .arg(id.to_string())
            .arg("/T")
            .arg("/F")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        // taskkill is normally quick, but a broken system utility must not
        // bypass the command's timeout or cancellation deadline.
        let _ = tokio::time::timeout(Duration::from_millis(750), killer).await;
    }
    let _ = child.start_kill();
}

fn append_process_line(
    output: &mut String,
    total_bytes: &mut usize,
    truncated: &mut bool,
    line: &ProcessLine,
    artifact_id: &str,
    sink: &Option<ToolEventSink>,
) {
    *total_bytes += line.text.len();
    if output.len() >= MAX_COMMAND_BYTES {
        *truncated = true;
        return;
    }
    let remaining = MAX_COMMAND_BYTES - output.len();
    let mut accepted = line.text.clone();
    if accepted.len() > remaining {
        let mut end = remaining;
        while !accepted.is_char_boundary(end) {
            end -= 1;
        }
        accepted.truncate(end);
        *truncated = true;
    }
    output.push_str(&accepted);
    if !accepted.is_empty() {
        if let Some(sink) = sink {
            sink(ToolEvent::TerminalOutput {
                artifact_id: artifact_id.into(),
                delta: accepted,
                stream: line.stream.clone(),
            });
        }
    }
}

async fn manage_task(
    args: &serde_json::Value,
    context: &ToolContext,
) -> Result<ToolExecutionResult, String> {
    let action = required_string(args, "action")?.trim().to_lowercase();
    let mgr = super::tasks::get_task_manager();
    match action.as_str() {
        "list" => {
            let tasks = mgr.list_tasks(&context.session_id).await;
            let formatted = if tasks.is_empty() {
                "No background tasks currently tracked.".to_string()
            } else {
                let mut s = String::new();
                for t in &tasks {
                    s.push_str(&format!(
                        "- [{}] {} | status: {} | pid: {} | log: {}\n",
                        t.id,
                        t.tool_summary.as_deref().unwrap_or(&t.command),
                        t.status,
                        t.pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".into()),
                        t.log_path
                    ));
                }
                s
            };
            Ok(ToolExecutionResult::success(formatted, None))
        }
        "status" => {
            let task_id = optional_string(args, "taskId", "task_id").ok_or_else(|| {
                "Missing required parameter 'taskId' for action 'status'".to_string()
            })?;
            let tail_lines = args
                .get("tailLines")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let info = mgr
                .get_status(&context.session_id, task_id, tail_lines)
                .await?;
            let formatted = format!(
                "Task: {}\nCommand: {}\nStatus: {}\nPID: {}\nStarted: {}ms\nCompleted: {}\nExit Code: {}\nLog Path: {}\n\n--- Recent Output ---\n{}",
                info.id,
                info.command,
                info.status,
                info.pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".into()),
                info.started_at,
                info.completed_at.map(|c| format!("{c}ms")).unwrap_or_else(|| "running".into()),
                info.exit_code.map(|e| e.to_string()).unwrap_or_else(|| "none".into()),
                info.log_path,
                if info.recent_output.is_empty() { "(no output yet)" } else { &info.recent_output }
            );
            Ok(ToolExecutionResult::success(formatted, None))
        }
        "send_input" => {
            let task_id = optional_string(args, "taskId", "task_id").ok_or_else(|| {
                "Missing required parameter 'taskId' for action 'send_input'".to_string()
            })?;
            let input = required_string(args, "input")?;
            let res = mgr.send_input(&context.session_id, task_id, input).await?;
            Ok(ToolExecutionResult::success(res, None))
        }
        "kill" => {
            let task_id = optional_string(args, "taskId", "task_id").ok_or_else(|| {
                "Missing required parameter 'taskId' for action 'kill'".to_string()
            })?;
            let res = mgr.kill_task(&context.session_id, task_id).await?;
            Ok(ToolExecutionResult::success(res, None))
        }
        _ => Err(format!(
            "Unknown action '{action}'. Supported actions: 'list', 'status', 'send_input', 'kill'"
        )),
    }
}

async fn fetch_web_page(
    args: &serde_json::Value,
    context: &ToolContext,
) -> Result<ToolExecutionResult, String> {
    let mut current = Url::parse(required_string(args, "url")?)
        .map_err(|error| format!("Invalid URL: {error}"))?;
    let max_chars = args
        .get("maxChars")
        .and_then(|v| v.as_u64())
        .unwrap_or(100_000)
        .clamp(2_000, 500_000) as usize;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("AetherAgent/0.2")
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = None;
    for _ in 0..=5 {
        validate_public_url(&current).await?;
        let next = tokio::select! {
            _ = context.cancel.cancelled() => return Ok(ToolExecutionResult { output: "Web fetch cancelled".into(), artifact: None, success: false, cancelled: true, ..Default::default() }),
            result = client.get(current.clone()).send() => result.map_err(|error| format!("Web fetch failed: {error}"))?
        };
        if next.status().is_redirection() {
            let location = next
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or("Redirect has no Location header")?;
            current = current
                .join(location)
                .map_err(|error| format!("Invalid redirect: {error}"))?;
            continue;
        }
        response = Some(next);
        break;
    }
    let response = response.ok_or("Too many redirects")?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Web page returned HTTP {status}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_WEB_BYTES as u64)
    {
        return Err("Web page exceeds 5 MiB".into());
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    loop {
        let next = tokio::select! {
            _ = context.cancel.cancelled() => return Ok(ToolExecutionResult { output: "Web fetch cancelled".into(), artifact: None, success: false, cancelled: true, ..Default::default() }),
            item = stream.next() => item,
        };
        let Some(item) = next else { break };
        let chunk = item.map_err(|error| format!("Web response stream failed: {error}"))?;
        if bytes.len() + chunk.len() > MAX_WEB_BYTES {
            return Err("Web page exceeded the 5 MiB response limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let raw = String::from_utf8_lossy(&bytes).to_string();
    let raw_for_html = raw.clone();
    let extracted = if content_type.contains("text/html")
        || raw.trim_start().starts_with("<!DOCTYPE")
        || raw.trim_start().starts_with("<html")
    {
        tokio::task::spawn_blocking(move || html2text::from_read(raw_for_html.as_bytes(), 100))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?
    } else {
        raw.clone()
    };
    let (mut extracted, truncated) = bounded_chars(extracted.trim(), max_chars);
    if extracted.trim().len() < 120 || extracted.contains("You need to enable JavaScript") {
        if let Some(meta_summary) = extract_meta_summary(&raw) {
            extracted = format!("{}\n\n[Page Metadata Summary]:\n{}", extracted.trim(), meta_summary);
        }
    }
    let artifact = if extracted.lines().count() > 25 {
        Some(new_artifact(
            format!("Web: {}", current.host_str().unwrap_or("page")),
            "markdown",
            Some("markdown".into()),
            extracted.clone(),
            Some(current.to_string()),
        ))
    } else {
        None
    };
    Ok(ToolExecutionResult::success(
        format!(
            "Fetched {}{}\n\n{}",
            current,
            if truncated { " (truncated)" } else { "" },
            extracted
        ),
        artifact,
    ))
}

async fn validate_public_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only HTTP(S) URLs are allowed".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs containing credentials are not allowed".into());
    }
    let host = url.host().ok_or("URL must contain a host")?;
    let ip = match host {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        url::Host::Domain(_) => None,
    };
    if let Some(ip) = ip {
        if !is_public_ip(ip) {
            return Err(
                "Private, loopback, link-local, and unspecified web targets are blocked".into(),
            );
        }
        return Ok(());
    }
    let host = url.host_str().ok_or("URL must contain a host")?;
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".local") {
        return Err("Local/private web targets are blocked".into());
    }
    let port = url
        .port_or_known_default()
        .ok_or("URL has no usable port")?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| format!("DNS lookup failed: {error}"))?;
    let mut found = false;
    for address in addresses {
        found = true;
        if !is_public_ip(address.ip()) {
            return Err("URL resolves to a private or local address".into());
        }
    }
    if found {
        Ok(())
    } else {
        Err("DNS returned no addresses".into())
    }
}

fn clean_html_text(raw: &str) -> String {
    let tag_re = match Regex::new(r"<[^>]+>") {
        Ok(re) => re.replace_all(raw, "").into_owned(),
        Err(_) => raw.to_string(),
    };
    tag_re
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn extract_meta_summary(raw_html: &str) -> Option<String> {
    let mut parts = Vec::new();
    if let Ok(re_title) = Regex::new(r#"(?is)<title[^>]*>(.*?)</title>"#) {
        if let Some(cap) = re_title.captures(raw_html) {
            let title = clean_html_text(cap.get(1).map(|m| m.as_str()).unwrap_or_default());
            if !title.is_empty() {
                parts.push(format!("Title: {title}"));
            }
        }
    }
    if let Ok(re_meta) = Regex::new(r#"(?is)<meta\s+[^>]*name=["']description["'][^>]*content=["'](.*?)["']"#) {
        if let Some(cap) = re_meta.captures(raw_html) {
            let desc = clean_html_text(cap.get(1).map(|m| m.as_str()).unwrap_or_default());
            if !desc.is_empty() {
                parts.push(format!("Description: {desc}"));
            }
        }
    }
    if let Ok(re_og) = Regex::new(r#"(?is)<meta\s+[^>]*property=["']og:description["'][^>]*content=["'](.*?)["']"#) {
        if let Some(cap) = re_og.captures(raw_html) {
            let og_desc = clean_html_text(cap.get(1).map(|m| m.as_str()).unwrap_or_default());
            if !og_desc.is_empty() && !parts.iter().any(|p| p.contains(&og_desc)) {
                parts.push(format!("OG Description: {og_desc}"));
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn decode_bing_u(raw_url: &str) -> String {
    let clean = raw_url.replace("&amp;", "&");
    if let Some(pos) = clean.find("u=a1") {
        let rest = &clean[pos + 4..];
        let token = rest.split('&').next().unwrap_or(rest);
        let mut normalized = token.replace('-', "+").replace('_', "/");
        while normalized.len() % 4 != 0 {
            normalized.push('=');
        }
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&normalized) {
            if let Ok(decoded) = String::from_utf8(bytes) {
                if decoded.starts_with("http://") || decoded.starts_with("https://") {
                    return decoded;
                }
            }
        }
    }
    clean
}

fn decode_ddg_url(raw_url: &str) -> String {
    let clean = raw_url.replace("&amp;", "&");
    if let Some(pos) = clean.find("uddg=") {
        let rest = &clean[pos + 5..];
        let token = rest.split('&').next().unwrap_or(rest);
        for (k, v) in url::form_urlencoded::parse(format!("u={token}").as_bytes()) {
            if k == "u" && (v.starts_with("http://") || v.starts_with("https://")) {
                return v.into_owned();
            }
        }
    }
    if clean.starts_with("//") {
        format!("https:{clean}")
    } else {
        clean
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub(super) fn parse_duckduckgo_results(html: &str, limit: usize) -> Vec<SearchResultItem> {
    let mut results = Vec::new();
    let re = match Regex::new(r#"(?s)<div[^>]*class="[^"]*result[^"]*"[^>]*>.*?<h2[^>]*class="result__title"[^>]*>.*?<a[^>]*href="([^"]+)"[^>]*>(.*?)</a>.*?</h2>.*?(?:<a[^>]*class="result__snippet"[^>]*>(.*?)</a>|<div[^>]*class="result__snippet"[^>]*>(.*?)</div>)"#) {
        Ok(r) => r,
        Err(_) => return results,
    };
    for cap in re.captures_iter(html) {
        if results.len() >= limit {
            break;
        }
        let raw_url = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
        let raw_title = cap.get(2).map(|m| m.as_str()).unwrap_or_default();
        let raw_snippet = cap.get(3).or_else(|| cap.get(4)).map(|m| m.as_str()).unwrap_or_default();

        let title = clean_html_text(raw_title);
        let snippet = clean_html_text(raw_snippet);
        let url = decode_ddg_url(raw_url);

        if !url.is_empty() && !title.is_empty() {
            results.push(SearchResultItem { title, url, snippet });
        }
    }
    results
}

pub(super) fn parse_bing_results(html: &str, limit: usize) -> Vec<SearchResultItem> {
    let mut results = Vec::new();
    let re = match Regex::new(r#"(?s)<li[^>]*class="b_algo"[^>]*>.*?<h2>\s*<a[^>]*href="([^"]+)"[^>]*>(.*?)</a>\s*</h2>.*?(?:<div[^>]*class="b_caption"[^>]*>.*?<p[^>]*>(.*?)</p>|<p[^>]*>(.*?)</p>)"#) {
        Ok(r) => r,
        Err(_) => return results,
    };
    for cap in re.captures_iter(html) {
        if results.len() >= limit {
            break;
        }
        let raw_url = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
        let raw_title = cap.get(2).map(|m| m.as_str()).unwrap_or_default();
        let raw_snippet = cap.get(3).or_else(|| cap.get(4)).map(|m| m.as_str()).unwrap_or_default();

        let title = clean_html_text(raw_title);
        let snippet = clean_html_text(raw_snippet);
        let url = decode_bing_u(raw_url);

        if !url.is_empty() && !title.is_empty() {
            results.push(SearchResultItem { title, url, snippet });
        }
    }
    results
}

pub(super) fn parse_duckduckgo_api_results(json: &serde_json::Value, limit: usize) -> Vec<SearchResultItem> {
    let mut results = Vec::new();
    if let (Some(text), Some(url)) = (
        json.get("AbstractText").and_then(|v| v.as_str()).filter(|s| !s.is_empty()),
        json.get("AbstractURL").and_then(|v| v.as_str()).filter(|s| !s.is_empty()),
    ) {
        let heading = json.get("Heading").and_then(|v| v.as_str()).unwrap_or("Abstract");
        results.push(SearchResultItem {
            title: heading.to_string(),
            url: url.to_string(),
            snippet: text.to_string(),
        });
    }
    if let Some(topics) = json.get("RelatedTopics").and_then(|v| v.as_array()) {
        for topic in topics {
            if results.len() >= limit {
                break;
            }
            if let (Some(text), Some(url)) = (
                topic.get("Text").and_then(|v| v.as_str()).filter(|s| !s.is_empty()),
                topic.get("FirstURL").and_then(|v| v.as_str()).filter(|s| !s.is_empty()),
            ) {
                let title = text.split(" - ").next().unwrap_or(text);
                results.push(SearchResultItem {
                    title: title.to_string(),
                    url: url.to_string(),
                    snippet: text.to_string(),
                });
            }
        }
    }
    results
}

async fn quick_fetch_snippet(
    client: &reqwest::Client,
    url_str: &str,
    context: &ToolContext,
) -> Result<String, String> {
    let Ok(url) = Url::parse(url_str) else {
        return Ok(String::new());
    };
    if validate_public_url(&url).await.is_err() {
        return Ok(String::new());
    }
    let resp = match tokio::select! {
        _ = context.cancel.cancelled() => return Ok(String::new()),
        res = client.get(url_str).timeout(Duration::from_secs(4)).send() => res,
    } {
        Ok(r) => r,
        Err(_) => return Ok(String::new()),
    };
    if !resp.status().is_success() {
        return Ok(String::new());
    }
    let Ok(raw) = resp.text().await else {
        return Ok(String::new());
    };
    let clean = clean_html_text(&raw);
    let trimmed = clean.chars().take(300).collect::<String>();
    Ok(trimmed)
}

async fn search_web(
    args: &serde_json::Value,
    context: &ToolContext,
) -> Result<ToolExecutionResult, String> {
    let query = required_string(args, "query")?.trim();
    if query.is_empty() {
        return Err("Search query cannot be empty".into());
    }
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .clamp(1, 10) as usize;

    let domain = args
        .get("domain")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let effective_query = if let Some(dom) = domain {
        format!("{query} site:{dom}")
    } else {
        query.to_string()
    };

    let fetch_content = args
        .get("fetch_content")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let encoded_query = url::form_urlencoded::byte_serialize(effective_query.as_bytes()).collect::<String>();
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|error| error.to_string())?;

    let mut results = Vec::new();

    // 1. Try DuckDuckGo HTML
    let ddg_url = format!("https://html.duckduckgo.com/html/?q={encoded_query}");
    let ddg_fetch = tokio::select! {
        _ = context.cancel.cancelled() => return Ok(ToolExecutionResult { output: "Search cancelled".into(), artifact: None, success: false, cancelled: true, ..Default::default() }),
        res = client.get(&ddg_url).send() => res,
    };

    if let Ok(resp) = ddg_fetch {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                results = parse_duckduckgo_results(&text, limit);
            }
        }
    }

    // 2. Fallback to Bing Search HTML (essential when DuckDuckGo is blocked or unreachable)
    if results.is_empty() {
        let bing_url = format!("https://www.bing.com/search?q={encoded_query}");
        let bing_fetch = tokio::select! {
            _ = context.cancel.cancelled() => return Ok(ToolExecutionResult { output: "Search cancelled".into(), artifact: None, success: false, cancelled: true, ..Default::default() }),
            res = client.get(&bing_url).send() => res,
        };
        if let Ok(resp) = bing_fetch {
            if resp.status().is_success() {
                if let Ok(text) = resp.text().await {
                    results = parse_bing_results(&text, limit);
                }
            }
        }
    }

    // 3. Fallback to DuckDuckGo Instant Answer API (JSON API, non-blocked)
    if results.is_empty() {
        let ddg_api_url = format!("https://api.duckduckgo.com/?q={encoded_query}&format=json&no_html=1&skip_disambig=1");
        if let Ok(resp) = client.get(&ddg_api_url).send().await {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    results = parse_duckduckgo_api_results(&json, limit);
                }
            }
        }
    }

    if results.is_empty() {
        return Ok(ToolExecutionResult::success(
            format!("No web search results found for query: \"{effective_query}\""),
            None,
        ));
    }

    // Optional quick fetch snippet for top results if fetch_content is enabled
    if fetch_content && !results.is_empty() {
        let top_count = results.len().min(2);
        for i in 0..top_count {
            if let Ok(content) = quick_fetch_snippet(&client, &results[i].url, context).await {
                if !content.is_empty() {
                    results[i].snippet = format!("{}\n   Content extract: {}", results[i].snippet, content);
                }
            }
        }
    }

    let mut output = format!("Search results for \"{effective_query}\":\n\n");
    for (idx, item) in results.iter().enumerate() {
        output.push_str(&format!("{}. [{}]({})\n   {}\n\n", idx + 1, item.title, item.url, item.snippet));
    }

    let artifact = if results.len() >= 3 {
        Some(new_artifact(
            format!("Search: {effective_query}"),
            "markdown",
            Some("markdown".into()),
            output.clone(),
            Some(format!("search://{encoded_query}")),
        ))
    } else {
        None
    };

    Ok(ToolExecutionResult::success(output.trim().to_string(), artifact))
}

async fn ask_question(args: &serde_json::Value) -> Result<ToolExecutionResult, String> {
    let questions = args
        .get("questions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Missing or invalid required 'questions' array".to_string())?;

    if questions.is_empty() {
        return Err("The 'questions' array cannot be empty".to_string());
    }

    let mut formatted = String::from("Interactive question(s) presented to user:\n");
    for (i, q) in questions.iter().enumerate() {
        let question_text = q
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Question");
        let is_multi = q
            .get("is_multi_select")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        formatted.push_str(&format!(
            "\n{}. {} (selection: {})\n",
            i + 1,
            question_text,
            if is_multi { "multiple" } else { "single" }
        ));
        if let Some(opts) = q.get("options").and_then(|v| v.as_array()) {
            for opt in opts {
                if let Some(opt_text) = opt.as_str() {
                    formatted.push_str(&format!("   - {}\n", opt_text));
                }
            }
        }
    }

    Ok(ToolExecutionResult {
        output: formatted.trim_end().to_string(),
        artifact: None,
        success: true,
        cancelled: false,
        duration_ms: Some(5),
        exit_code: Some(0),
        terminal_output: None,
        files_changed: None,
    })
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_documentation()
                || octets[0] >= 240 // 240.0.0.0/4 reserved for future use
                || (octets[0] == 100 && (64..=127).contains(&octets[1])) // 100.64.0.0/10 CGNAT
                || (octets[0] == 198 && (18..=19).contains(&octets[1])) // 198.18.0.0/15 benchmarking
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0) // 192.0.0.0/24
                || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)) // 192.88.99.0/24)
        }
        IpAddr::V6(ip) => {
            // IPv4-mapped (::ffff:0:0/96), 6to4 (2002::/16) and Teredo
            // (2001::/32) addresses wrap an IPv4 target, so the embedded
            // address must face the same filter instead of the IPv6 wrapper.
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(mapped));
            }
            let segments = ip.segments();
            if segments[0] == 0x2001 && segments[1] == 0x0db8 {
                // 2001:db8::/32 is documentation-only.
                return false;
            }
            if segments[0] == 0x0064 && segments[1] == 0xff9b {
                // 64:ff9b::/96 (NAT64 well-known prefix) embeds an IPv4
                // target, which must face the same filter.
                let embedded = Ipv4Addr::new(
                    (segments[6] >> 8) as u8,
                    segments[6] as u8,
                    (segments[7] >> 8) as u8,
                    segments[7] as u8,
                );
                return is_public_ip(IpAddr::V4(embedded));
            }
            if segments[0] == 0x2002 {
                let embedded = Ipv4Addr::new(
                    (segments[1] >> 8) as u8,
                    segments[1] as u8,
                    (segments[2] >> 8) as u8,
                    segments[2] as u8,
                );
                return is_public_ip(IpAddr::V4(embedded));
            }
            if segments[0] == 0x2001 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0 {
                let server = Ipv4Addr::new(
                    (segments[4] >> 8) as u8,
                    segments[4] as u8,
                    (segments[5] >> 8) as u8,
                    segments[5] as u8,
                );
                return is_public_ip(IpAddr::V4(server));
            }
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_multicast())
        }
    }
}

pub(super) fn workspace_relative_path(cwd: &str, path: &Path) -> Result<String, String> {
    let root =
        std::fs::canonicalize(cwd).map_err(|error| format!("Workspace is unavailable: {error}"))?;
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .map_err(|_| "Resolved path cannot be made relative to the active workspace".into())
}

pub fn resolve_workspace_path(cwd: &str, requested: &str) -> Result<PathBuf, String> {
    if requested.contains('\0') || requested.starts_with(r"\\?\") || requested.starts_with(r"\\.\")
    {
        return Err("NT device/namespace paths are not allowed".into());
    }
    if cwd.trim().is_empty() {
        return Err(
            "No active workspace selected. Choose a workspace before using workspace file tools."
                .into(),
        );
    }
    let root =
        std::fs::canonicalize(cwd).map_err(|error| format!("Workspace is unavailable: {error}"))?;
    if !root.is_dir() {
        return Err("Workspace is not a directory".into());
    }
    let raw = Path::new(requested);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        validate_relative_path(requested)?;
        root.join(raw)
    };
    if candidate
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("Parent traversal is not allowed; use a workspace-contained path".into());
    }
    if candidate.exists() {
        let resolved = std::fs::canonicalize(&candidate)
            .map_err(|error| format!("Cannot resolve {}: {error}", candidate.display()))?;
        if !resolved.starts_with(&root) {
            return Err("Path escapes the active workspace".into());
        }
        return Ok(resolved);
    }
    let mut ancestor = candidate.parent().unwrap_or(&root);
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or("Cannot find an existing target ancestor")?;
    }
    let resolved_ancestor = std::fs::canonicalize(ancestor).map_err(|error| error.to_string())?;
    if !resolved_ancestor.starts_with(&root) {
        return Err("Target parent escapes the active workspace".into());
    }
    Ok(candidate)
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    for part in path.replace('\\', "/").split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err("Parent traversal is not allowed".into());
        }
        if part.contains(':') || part.ends_with('.') || part.ends_with(' ') {
            return Err("Path contains a Windows-unsafe component".into());
        }
        let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        ) {
            return Err("Windows reserved device names are not allowed".into());
        }
    }
    Ok(())
}

pub async fn list_workspace_files(
    cwd: &str,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<String>, String> {
    if cwd.trim().is_empty() {
        return Ok(Vec::new());
    }
    let root = resolve_workspace_path(cwd, ".")?;
    let query = query.unwrap_or_default().to_ascii_lowercase();
    let limit = limit.clamp(1, 200);
    tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        for entry in WalkBuilder::new(&root)
            .hidden(false)
            .filter_entry(|entry| {
                if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
                    return true;
                }
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                !matches!(
                    name.as_str(),
                    ".git"
                        | "node_modules"
                        | "target"
                        | "dist"
                        | "dist-ssr"
                        | "build"
                        | "out"
                        | "coverage"
                        | ".next"
                )
            })
            .build()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            if relative.to_ascii_lowercase().contains(&query) {
                files.push(relative);
                if files.len() >= limit {
                    break;
                }
            }
        }
        files.sort();
        files
    })
    .await
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> PathBuf {
        let path = std::env::temp_dir().join(format!("aether-tools-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[tokio::test]
    async fn nested_mutations_report_workspace_relative_path() {
        let root = workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("config.ts"), "root").unwrap();
        std::fs::write(root.join("src/config.ts"), "nested").unwrap();
        let cwd = root.to_string_lossy();
        let written = write_file(
            &serde_json::json!({"path":"src/config.ts","content":"first"}),
            &cwd,
        )
        .await
        .unwrap();
        assert_eq!(written.files_changed.unwrap()[0].path, "src/config.ts");
        let edited = edit_file(
            &serde_json::json!({"path":"src/config.ts","oldString":"first","newString":"second"}),
            &cwd,
        )
        .await
        .unwrap();
        assert_eq!(edited.files_changed.unwrap()[0].path, "src/config.ts");
        assert_eq!(
            std::fs::read_to_string(root.join("config.ts")).unwrap(),
            "root"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn mutations_wait_for_same_file_lock_before_reading() {
        let root = workspace();
        let file = root.join("shared.txt");
        std::fs::write(&file, "one two").unwrap();
        let resolved = std::fs::canonicalize(&file).unwrap();
        let held = lock_file_mutation(&resolved).await.unwrap();
        let cwd = root.to_string_lossy().to_string();
        let mut editing = tokio::spawn(async move {
            edit_file(
                &serde_json::json!({"path":"shared.txt","oldString":"two","newString":"three"}),
                &cwd,
            )
            .await
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut editing)
                .await
                .is_err()
        );
        std::fs::write(&file, "updated two").unwrap();
        drop(held);
        editing.await.unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "updated three");
        let cwd = root.to_string_lossy();
        let a = serde_json::json!({"path":"shared.txt","oldString":"updated","newString":"first"});
        let b = serde_json::json!({"path":"shared.txt","oldString":"three","newString":"second"});
        let (a, b) = tokio::join!(edit_file(&a, &cwd), edit_file(&b, &cwd));
        a.unwrap();
        b.unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "first second");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_traversal_and_accepts_nested_workspace_path() {
        let root = workspace();
        let root_string = root.to_string_lossy().to_string();
        assert!(resolve_workspace_path(&root_string, ".").is_ok());
        assert!(resolve_workspace_path(&root_string, "../escape.txt").is_err());
        assert!(resolve_workspace_path(&root_string, "nested/ok.txt").is_ok());
        assert!(resolve_workspace_path(&root_string, "nul.txt").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_workspace_never_grants_absolute_file_access() {
        assert!(resolve_workspace_path("", "D:/outside.txt").is_err());
        assert!(resolve_workspace_path("", "/etc/passwd").is_err());
        assert!(resolve_workspace_path("", "relative.txt").is_err());
    }

    #[test]
    fn process_utf8_decoder_preserves_split_characters() {
        let mut bytes = "héllo".as_bytes().to_vec();
        let split = bytes.iter().position(|byte| *byte == 0xC3).unwrap() + 1;
        let tail = bytes.split_off(split);
        assert_eq!(take_utf8_output(&mut bytes, false), "h");
        bytes.extend_from_slice(&tail);
        assert_eq!(take_utf8_output(&mut bytes, true), "éllo");
    }

    #[test]
    fn symlink_escape_is_rejected() {
        #[cfg(unix)]
        {
            let root = workspace();
            let outside = workspace();
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            assert!(resolve_workspace_path(&root.to_string_lossy(), "link/secret.txt").is_err());
            let _ = std::fs::remove_dir_all(root);
            let _ = std::fs::remove_dir_all(outside);
        }
    }

    #[test]
    fn edit_projected_utf8_length_checks_limits_and_overflow() {
        let limit = MAX_FILE_BYTES as usize;
        assert_eq!(checked_edit_result_len(limit, 1, 1, 1), Ok(limit));
        assert_eq!(checked_edit_result_len(8, 2, 3, 1), Ok(4));
        assert_eq!(checked_edit_result_len(4, 2, "é".len(), "界".len()), Ok(6));
        assert_eq!(checked_edit_result_len(4, 2, 2, 0), Ok(0));
        for lengths in [
            (limit, 1, 1, 2),
            (usize::MAX, 2, usize::MAX, 0),
            (2, 2, 1, usize::MAX),
            (usize::MAX, 1, 1, 2),
            (1, 2, 1, 0),
        ] {
            assert_eq!(
                checked_edit_result_len(lengths.0, lengths.1, lengths.2, lengths.3),
                Err("edit_file result exceeds the 10 MiB limit".into()),
                "{lengths:?}"
            );
        }
    }

    #[tokio::test]
    async fn edit_builds_one_diff_and_preserves_output_contract() {
        let root = workspace();
        for (original, old, new, replace_all, updated, additions, deletions) in [
            ("one\ntwo\n", "two", "three", false, "one\nthree\n", 1, 1),
            ("é\nkeep\né", "é", "界\nplus", true, "界\nplus\nkeep\n界\nplus", 4, 2),
            ("one\ntwo\n", "two\n", "", false, "one\n", 0, 1),
            ("same\n", "same", "same", false, "same\n", 0, 0),
        ] {
            std::fs::write(root.join("a.txt"), original).unwrap();
            let args = serde_json::json!({
                "path": "a.txt", "oldString": old, "newString": new, "replaceAll": replace_all
            });
            let (result, builds) = EDIT_DIFF_BUILDS
                .scope(std::cell::Cell::new(0), async {
                    let result = edit_file(&args, &root.to_string_lossy()).await.unwrap();
                    (result, EDIT_DIFF_BUILDS.with(std::cell::Cell::get))
                })
                .await;
            let expected_diff = TextDiff::from_lines(original, updated)
                .unified_diff()
                .context_radius(3)
                .header("a/a.txt", "b/a.txt")
                .to_string();
            assert_eq!(result.artifact.as_ref().unwrap().content, expected_diff);
            assert_eq!(result.artifact.as_ref().unwrap().title, "Diff: a.txt");
            let stats = &result.files_changed.as_ref().unwrap()[0];
            assert_eq!((&*stats.path, stats.additions, stats.deletions), ("a.txt", additions, deletions));
            assert!(result.success);
            assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), updated);
            if original == "one\ntwo\n" && new == "three" {
                assert_eq!(expected_diff, "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+three\n");
            }
            if builds != 1 {
                std::fs::remove_dir_all(&root).unwrap();
            }
            assert_eq!(builds, 1, "edit_file must compute the line diff only once");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn edit_rejects_expansion_before_allocating_and_preserves_file() {
        let root = workspace();
        let file = root.join("expand.txt");
        let original = "x".repeat(4096);
        std::fs::write(&file, &original).unwrap();
        let args = serde_json::json!({
            "path": "expand.txt",
            "oldString": "x",
            "newString": "é".repeat(2048),
            "replaceAll": true
        });
        let attempt = tokio::spawn(async move {
            let result = FORBID_EDIT_ALLOCATION
                .scope(true, edit_file(&args, &root.to_string_lossy()))
                .await;
            (root, result)
        })
        .await;
        let unchanged = std::fs::read_to_string(&file).unwrap() == original;
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
        assert!(unchanged, "rejected expansion changed the original file");
        let (_, result) = attempt.expect("oversized edits must reject before replacement allocation");
        assert_eq!(result.unwrap_err(), "edit_file result exceeds the 10 MiB limit");
    }

    #[tokio::test]
    async fn exact_edit_generates_diff_and_writes_once() {
        let root = workspace();
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        let result = edit_file(
            &serde_json::json!({"path":"a.txt","oldString":"two","newString":"three"}),
            &root.to_string_lossy(),
        )
        .await
        .unwrap();
        assert!(result.artifact.unwrap().content.contains("+three"));
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "one\nthree\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn empty_file_read_is_successful() {
        let root = workspace();
        std::fs::write(root.join("empty.txt"), "").unwrap();
        let result = read_file(
            &serde_json::json!({"path":"empty.txt"}),
            &root.to_string_lossy(),
        )
        .await
        .unwrap();
        assert!(result.success);
        assert!(result.output.contains("empty file"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn context_file_listing_excludes_generated_and_vcs_directories() {
        let root = workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("src/main.ts"), "export {};\n").unwrap();
        std::fs::write(root.join(".git/config"), "secret").unwrap();
        std::fs::write(root.join("node_modules/pkg/index.js"), "module").unwrap();
        let files = list_workspace_files(&root.to_string_lossy(), None, 50)
            .await
            .unwrap();
        assert_eq!(files, vec!["src/main.ts"]);
        let _ = std::fs::remove_dir_all(root);
    }

    fn assert_foreground_poll_budget(probe: &ForegroundProbe, result: &ToolExecutionResult) {
        assert!(probe
            .closed_while_running
            .load(std::sync::atomic::Ordering::Relaxed));
        let polls = probe.polls.load(std::sync::atomic::Ordering::Relaxed);
        let duration_ms = result.duration_ms.unwrap();
        let budget = duration_ms / 50 + 6;
        eprintln!("foreground closed output: polls={polls}, duration_ms={duration_ms}, budget={budget}");
        assert!(
            polls as u64 <= budget,
            "closed output busy-spun: {polls} polls, budget {budget}"
        );
    }

    #[tokio::test]
    async fn foreground_closed_output_waits_without_spinning() {
        let root = workspace();
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd: root.to_string_lossy().to_string(),
            cancel: CancellationToken::new(),
            sink: None,
        };
        let command = if cfg!(windows) {
            "Start-Sleep -Milliseconds 750; exit 0"
        } else {
            "sleep 0.75; exit 0"
        };
        let probe = Arc::new(ForegroundProbe::default());
        let args = serde_json::json!({"command": command, "timeoutSeconds": 5});
        let result = tokio::time::timeout(
            Duration::from_secs(8),
            FOREGROUND_PROBE.scope(probe.clone(), run_command(&args, &context)),
        )
        .await;
        std::fs::remove_dir_all(root).unwrap();
        let result = result
            .expect("closed output must not prevent process completion")
            .unwrap();
        assert!(result.success, "{}", result.output);
        assert!(!result.cancelled);
        assert_eq!(result.exit_code, Some(0));
        assert!(
            result.duration_ms.unwrap() >= 600,
            "the isolated child must really sleep"
        );
        assert_eq!(result.terminal_output.as_deref(), Some(""));
        assert_foreground_poll_budget(&probe, &result);
    }

    #[tokio::test]
    async fn foreground_closed_output_respects_timeout() {
        let root = workspace();
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd: root.to_string_lossy().to_string(),
            cancel: CancellationToken::new(),
            sink: None,
        };
        let command = if cfg!(windows) {
            "Start-Sleep -Seconds 30"
        } else {
            "exec sleep 30"
        };
        let probe = Arc::new(ForegroundProbe::default());
        let args = serde_json::json!({"command": command, "timeoutSeconds": 1});
        let result = tokio::time::timeout(
            Duration::from_secs(8),
            FOREGROUND_PROBE.scope(probe.clone(), run_command(&args, &context)),
        )
        .await;
        std::fs::remove_dir_all(root).unwrap();
        let result = result
            .expect("closed output must not delay the deadline")
            .unwrap();
        assert!(!result.success);
        assert!(!result.cancelled);
        assert_eq!(result.exit_code, Some(1));
        assert!(result
            .output
            .contains("Command timed out after 1s and was terminated."));
        assert!(
            (1000..4000).contains(&result.duration_ms.unwrap()),
            "timeout duration was {:?} ms",
            result.duration_ms
        );
        assert_foreground_poll_budget(&probe, &result);
    }

    #[tokio::test]
    async fn foreground_closed_output_remains_cancellable() {
        let root = workspace();
        let cancel = CancellationToken::new();
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd: root.to_string_lossy().to_string(),
            cancel: cancel.clone(),
            sink: None,
        };
        let command = if cfg!(windows) {
            "Start-Sleep -Seconds 30"
        } else {
            "exec sleep 30"
        };
        let probe = Arc::new(ForegroundProbe::default());
        let captured = probe.clone();
        let task = tokio::spawn(async move {
            let args = serde_json::json!({"command": command, "timeoutSeconds": 60});
            FOREGROUND_PROBE
                .scope(captured, run_command(&args, &context))
                .await
        });
        let ready = tokio::time::timeout(Duration::from_secs(5), async {
            while !probe
                .closed_while_running
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        })
        .await;
        let cancelled_at = Instant::now();
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(8), task).await;
        std::fs::remove_dir_all(root).unwrap();
        ready.expect("the live child must be observed after output closes");
        let result = result
            .expect("closed output must remain cancellable")
            .unwrap()
            .unwrap();
        let cancel_latency_ms = cancelled_at.elapsed().as_millis();
        eprintln!("foreground cancellation latency_ms={cancel_latency_ms}");
        assert!(cancel_latency_ms < 3000);
        assert!(result.cancelled);
        assert!(!result.success);
        assert_eq!(result.exit_code, Some(1));
        assert!(result.output.contains("Command cancelled by user."));
        assert!(!result.output.contains("timed out"));
        assert_foreground_poll_budget(&probe, &result);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn powershell_streams_output_and_finishes() {
        let root = workspace();
        let events = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured = events.clone();
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd: root.to_string_lossy().to_string(),
            cancel: CancellationToken::new(),
            sink: Some(Arc::new(move |event| {
                if let ToolEvent::TerminalOutput { delta, .. } = event {
                    captured.lock().unwrap().push(delta);
                }
            })),
        };
        let result = run_command(
            &serde_json::json!({"command":"Write-Output 'hello-aether'","timeoutSeconds":20}),
            &context,
        )
        .await
        .unwrap();
        assert!(result.success, "{}", result.output);
        assert!(result.output.contains("hello-aether"));
        assert!(events.lock().unwrap().join("").contains("hello-aether"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn powershell_giant_unbroken_output_is_bounded() {
        let root = workspace();
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd: root.to_string_lossy().to_string(),
            cancel: CancellationToken::new(),
            sink: None,
        };
        let result = run_command(
            &serde_json::json!({
                "command": "[Console]::Out.Write('a' * (2 * 1024 * 1024))",
                "timeoutSeconds": 20
            }),
            &context,
        )
        .await
        .unwrap();
        assert!(result.success, "{}", result.output);
        assert!(result.output.contains("terminal output truncated at 1 MiB"));
        assert!(result.artifact.unwrap().content.len() <= MAX_COMMAND_BYTES + 128);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn powershell_cancellation_terminates_the_process() {
        let root = workspace();
        let cancel = CancellationToken::new();
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd: root.to_string_lossy().to_string(),
            cancel: cancel.clone(),
            sink: None,
        };
        let task = tokio::spawn(async move {
            run_command(&serde_json::json!({"command":"Start-Sleep -Seconds 30; Write-Output 'unreachable'","timeoutSeconds":60}), &context).await
        });
        tokio::time::sleep(Duration::from_millis(250)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(8), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(result.cancelled);
        assert!(!result.output.contains("unreachable"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn private_addresses_are_blocked() {
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("10.0.0.1".parse().unwrap()));
        assert!(is_public_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn ipv4_mapped_and_embedded_ipv6_targets_cannot_bypass_the_private_filter() {
        // IPv4-mapped (::ffff:0:0/96) addresses must be filtered as their IPv4 selves.
        assert!(!is_public_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("::ffff:10.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("::ffff:169.254.169.254".parse().unwrap()));
        // A genuinely public IPv4-mapped address stays allowed.
        assert!(is_public_ip("::ffff:8.8.8.8".parse().unwrap()));
        // 6to4 (2002::/16) and Teredo (2001::/32) embed an IPv4 target that
        // must face the same filter instead of being treated as public.
        assert!(!is_public_ip("2002:7f00:0001::".parse().unwrap()));
        assert!(is_public_ip("2002:0808:0808::".parse().unwrap()));
        assert!(!is_public_ip(
            "2001:0000:0000:0000:0000:0000:7f00:0001".parse().unwrap()
        ));
    }

    #[tokio::test]
    async fn fetch_rejects_ipv4_mapped_loopback_literal_urls() {
        let error = validate_public_url(&Url::parse("http://[::ffff:127.0.0.1]/").unwrap())
            .await
            .expect_err("IPv4-mapped loopback must be rejected");
        assert!(
            error.contains("private") || error.contains("local"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn fetch_rejects_private_ipv6_literal_urls_before_dns() {
        for target in [
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:7f00:1]:8080/",
            "https://[::ffff:10.0.0.1]/",
        ] {
            let error = validate_public_url(&Url::parse(target).unwrap())
                .await
                .expect_err("private IPv6 literals must be rejected before DNS");
            assert_eq!(
                error, "Private, loopback, link-local, and unspecified web targets are blocked",
                "{target}"
            );
        }
    }

    #[tokio::test]
    async fn fetch_accepts_public_ip_literal_urls_without_dns() {
        for target in [
            "https://[2606:4700:4700::1111]/",
            "http://[::ffff:8.8.8.8]:8080/",
            "https://8.8.8.8/",
        ] {
            assert_eq!(
                validate_public_url(&Url::parse(target).unwrap()).await,
                Ok(()),
                "{target}"
            );
        }
    }

    #[test]
    fn agent_tools_contain_subagent_and_skill_contracts() {
        let tools = get_agent_tools();
        let array = tools.as_array().expect("tools array");
        let names: Vec<&str> = array
            .iter()
            .filter_map(|t| t.get("function")?.get("name")?.as_str())
            .collect();
        for name in ["skills_list", "skill_view", "skill_manage"] {
            assert_eq!(
                names.iter().filter(|&&n| n == name).count(),
                1,
                "missing or duplicate {name}"
            );
        }
        assert!(names.contains(&"spawn_subagent"));
        assert!(names.contains(&"use_skill"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"run_command"));
        assert!(names.contains(&"manage_task"));
    }

    #[tokio::test]
    async fn manage_task_roundtrip_with_daemon() {
        let root = workspace();
        let cancel = CancellationToken::new();
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd: root.to_string_lossy().to_string(),
            cancel,
            sink: None,
        };

        let cmd = if cfg!(windows) {
            "Start-Sleep -Seconds 20"
        } else {
            "sleep 20"
        };

        // 1. Start daemon command
        let spawn_res = run_command(
            &serde_json::json!({
                "command": cmd,
                "isDaemon": true,
                "toolSummary": "Test background watcher"
            }),
            &context,
        )
        .await
        .expect("spawn daemon");
        assert!(spawn_res.success);
        assert!(spawn_res
            .output
            .contains("Background task started successfully"));

        // Extract taskId
        let task_id = spawn_res
            .output
            .lines()
            .find(|l| l.starts_with("TaskId: "))
            .map(|l| l.trim_start_matches("TaskId: ").trim())
            .expect("task id in output");

        // 2. Test manage_task list
        let list_res = manage_task(&serde_json::json!({"action": "list"}), &context)
            .await
            .expect("list tasks");
        assert!(list_res.output.contains(task_id));

        // 3. Test manage_task status
        let status_res = manage_task(
            &serde_json::json!({
                "action": "status",
                "taskId": task_id
            }),
            &context,
        )
        .await
        .expect("status task");
        assert!(status_res.output.contains(task_id));
        assert!(status_res.output.contains("Status: running"));

        // 4. Test manage_task kill
        let kill_res = manage_task(
            &serde_json::json!({
                "action": "kill",
                "taskId": task_id
            }),
            &context,
        )
        .await
        .expect("kill task");
        assert!(kill_res.output.contains("terminated"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn guarded_write_rejects_stale_or_unexpected_disk_state() {
        let root = workspace();
        let file = root.join("guarded.txt");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        guarded_atomic_write(&file, b"one\nthree\n", Some("one\ntwo\n"))
            .await
            .expect("unchanged snapshot must be writable");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nthree\n");
        let error = guarded_atomic_write(&file, b"clobbered", Some("one\ntwo\n"))
            .await
            .expect_err("stale snapshot must be rejected");
        assert!(
            error.contains("changed") || error.contains("appeared"),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nthree\n");
        let error = guarded_atomic_write(&file, b"clobbered", None)
            .await
            .expect_err("unexpected existing file must be rejected");
        assert!(error.contains("appeared"), "{error}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nthree\n");
        let fresh = root.join("fresh.txt");
        guarded_atomic_write(&fresh, b"new", None)
            .await
            .expect("fresh target must be writable");
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "new");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn write_file_reports_accurate_diff_stats_for_overwrite() {
        let root = workspace();
        std::fs::write(root.join("w.txt"), "a\nb\nc\n").unwrap();
        let result = write_file(
            &serde_json::json!({"path":"w.txt","content":"a\n"}),
            &root.to_string_lossy(),
        )
        .await
        .unwrap();
        let stats = result.files_changed.expect("files changed stats");
        assert_eq!(
            stats[0].additions, 0,
            "the surviving line is unchanged, so it is not an addition"
        );
        assert_eq!(stats[0].deletions, 2, "two overwritten lines are deletions");
        let empty = write_file(
            &serde_json::json!({"path":"e.txt","content":""}),
            &root.to_string_lossy(),
        )
        .await
        .unwrap();
        let stats = empty.files_changed.expect("files changed stats");
        assert_eq!(stats[0].additions, 0);
        assert_eq!(stats[0].deletions, 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reserved_benchmark_and_shared_ipv4_ranges_are_blocked() {
        assert!(!is_public_ip("240.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("255.255.255.254".parse().unwrap()));
        assert!(!is_public_ip("100.64.0.1".parse().unwrap()));
        assert!(!is_public_ip("100.127.255.255".parse().unwrap()));
        assert!(!is_public_ip("198.18.0.1".parse().unwrap()));
        assert!(!is_public_ip("198.19.255.255".parse().unwrap()));
        assert!(!is_public_ip("192.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("192.88.99.1".parse().unwrap()));
        assert!(is_public_ip("100.63.255.255".parse().unwrap()));
        assert!(is_public_ip("100.128.0.1".parse().unwrap()));
        assert!(is_public_ip("198.20.0.1".parse().unwrap()));
    }

    #[test]
    fn ipv6_documentation_and_nat64_targets_cannot_bypass_the_private_filter() {
        assert!(!is_public_ip("2001:db8::1".parse().unwrap()));
        assert!(!is_public_ip("64:ff9b::7f00:1".parse().unwrap()));
        assert!(!is_public_ip("64:ff9b::a00:1".parse().unwrap()));
        assert!(is_public_ip("64:ff9b::808:808".parse().unwrap()));
    }

    #[test]
    fn bing_search_results_and_u_decoding_work() {
        let sample = r#"
            <li class="b_algo">
                <h2><a href="https://www.bing.com/ck/a?!&amp;&amp;u=a1aHR0cHM6Ly9ydXN0LWxhbmcub3JnLw&amp;ntb=1">Rust <b>Lang</b></a></h2>
                <div class="b_caption"><p>Empowering everyone to build reliable software.</p></div>
            </li>
        "#;
        let results = parse_bing_results(sample, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Lang");
        assert_eq!(results[0].url, "https://rust-lang.org/");
        assert_eq!(results[0].snippet, "Empowering everyone to build reliable software.");
    }

    #[test]
    fn duckduckgo_results_and_uddg_decoding_work() {
        let sample = r##"
            <div class="result results_links">
                <h2 class="result__title"><a class="result__url" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F&rut=1">Rust &amp; Language</a></h2>
                <a class="result__snippet" href="#">A language empowering everyone.</a>
            </div>
        "##;
        let results = parse_duckduckgo_results(sample, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust & Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(results[0].snippet, "A language empowering everyone.");
    }

    #[tokio::test]
    async fn ask_question_formats_output_properly() {
        let args = serde_json::json!({
            "questions": [
                {
                    "question": "Which database would you prefer?",
                    "options": ["SQLite", "PostgreSQL"],
                    "is_multi_select": false
                }
            ]
        });
        let result = ask_question(&args).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("Which database would you prefer?"));
        assert!(result.output.contains("SQLite"));
        assert!(result.output.contains("PostgreSQL"));

        let tools = get_agent_tools();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"ask_question"));
    }

    #[test]
    fn duckduckgo_api_results_parse_properly() {
        let json = serde_json::json!({
            "Heading": "Rust (programming language)",
            "AbstractText": "Rust is a general-purpose programming language emphasizing performance, type safety, and concurrency.",
            "AbstractURL": "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "RelatedTopics": [
                {
                    "Text": "Cargo - The Rust package manager",
                    "FirstURL": "https://doc.rust-lang.org/cargo/"
                }
            ]
        });
        let results = parse_duckduckgo_api_results(&json, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust (programming language)");
        assert_eq!(results[0].url, "https://en.wikipedia.org/wiki/Rust_(programming_language)");
        assert_eq!(results[1].title, "Cargo");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/cargo/");
    }

    #[test]
    fn extract_meta_summary_reads_title_and_descriptions() {
        let sample = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Tauri 2.0 Documentation</title>
                <meta name="description" content="Build smaller, faster, and more secure desktop applications.">
                <meta property="og:description" content="Official guides for Tauri v2 framework.">
            </head>
            <body><div id="root">You need to enable JavaScript to run this app.</div></body>
            </html>
        "#;
        let summary = extract_meta_summary(sample).expect("meta summary");
        assert!(summary.contains("Title: Tauri 2.0 Documentation"));
        assert!(summary.contains("Description: Build smaller, faster, and more secure desktop applications."));
        assert!(summary.contains("OG Description: Official guides for Tauri v2 framework."));
    }
}
