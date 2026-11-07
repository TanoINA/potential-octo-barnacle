use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::io::AsyncWriteExt;

mod agent;
use agent::runner::AgentRunner;
use agent::storage;
use agent::types::{
    AgentRunRequest, AgentSettings, McpServerConfig, ProviderConfig, Session, SessionData,
    SkillInfo,
};

pub struct AppState {
    pub workspace_cwd: Mutex<String>,
    pub runner: Arc<AgentRunner>,
}

#[tauri::command]
async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    request: AgentRunRequest,
) -> Result<(), String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    let runner = state.runner.clone();
    let cancel = runner.begin(&request.run_id, &request.session_id)?;
    let run_id = request.run_id.clone();
    tokio::spawn(async move {
        runner.run(app, cwd, request, cancel).await;
        runner.finish(&run_id);
    });
    Ok(())
}

#[tauri::command]
async fn send_council_message(
    app: AppHandle,
    state: State<'_, AppState>,
    request_a: AgentRunRequest,
    request_b: AgentRunRequest,
) -> Result<(), String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    let runner = state.runner.clone();
    let cancel = runner.begin(&request_a.run_id, &request_a.session_id)?;
    let run_id = request_a.run_id.clone();
    tokio::spawn(async move {
        runner
            .run_council(app, cwd, request_a, request_b, cancel)
            .await;
        runner.finish(&run_id);
    });
    Ok(())
}

#[tauri::command]
async fn review_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    provider: ProviderConfig,
) -> Result<(), String> {
    state.runner.review_session(app, session_id, provider).await
}

#[tauri::command]
async fn read_reflection_skill(
    session_id: String,
    skill_name: String,
    run_id: Option<String>,
) -> Result<String, String> {
    let data = storage::load_session(&session_id).await?;
    let review = data
        .messages
        .iter()
        .filter_map(|message| message.self_improvement.as_ref())
        .find(|review| {
            review.skill_name.as_deref() == Some(skill_name.as_str())
                && run_id.as_ref().is_none_or(|id| id == &review.run_id)
        })
        .ok_or("This skill is not associated with a review in the selected session")?;
    let cwd = review
        .workspace_cwd
        .as_ref()
        .or(data.session.workspace_cwd.as_ref())
        .ok_or("The session has no workspace")?;
    agent::reflection::read_generated_skill(cwd, &skill_name).await
}

#[tauri::command]
fn abort_agent(state: State<'_, AppState>, run_id: String) -> Result<(), String> {
    state.runner.abort(&run_id)
}

#[tauri::command]
async fn get_settings() -> Result<AgentSettings, String> {
    storage::load_settings().await
}

#[tauri::command]
async fn save_settings(settings: AgentSettings) -> Result<(), String> {
    storage::save_settings(&settings).await
}

#[tauri::command]
async fn list_sessions() -> Result<Vec<Session>, String> {
    Ok(storage::list_sessions().await)
}

#[tauri::command]
async fn load_session(session_id: String) -> Result<SessionData, String> {
    storage::load_session(&session_id).await
}

#[tauri::command]
async fn save_session(data: SessionData) -> Result<(), String> {
    storage::save_session(data).await
}

#[tauri::command]
async fn rename_session(session_id: String, title: String) -> Result<SessionData, String> {
    storage::rename_session(&session_id, &title).await
}

#[tauri::command]
async fn toggle_pin_session(session_id: String) -> Result<Session, String> {
    storage::toggle_pin_session(&session_id).await
}

#[tauri::command]
async fn list_all_artifacts() -> Result<Vec<agent::types::GlobalArtifactItem>, String> {
    Ok(storage::list_all_artifacts().await)
}

#[tauri::command]
async fn deep_search(
    query: String,
    limit: Option<usize>,
) -> Result<Vec<agent::search::DeepSearchResult>, String> {
    agent::search::search_query(&query, limit.unwrap_or(20))
}

#[tauri::command]
async fn delete_session(session_id: String) -> Result<(), String> {
    storage::delete_session(&session_id).await
}

#[tauri::command]
async fn export_session(
    state: State<'_, AppState>,
    session_id: String,
    destination: String,
    format: String,
) -> Result<(), String> {
    let requested = std::path::PathBuf::from(&destination);
    let resolved = if requested.is_absolute() {
        requested
    } else {
        let cwd = state
            .workspace_cwd
            .lock()
            .map_err(|_| "Workspace lock is unavailable".to_string())?
            .clone();
        if cwd.is_empty() {
            return Err("A relative export path requires an active workspace.".into());
        }
        std::path::Path::new(&cwd).join(requested)
    };
    storage::export_session(&session_id, &resolved.to_string_lossy(), &format).await
}

#[tauri::command]
async fn compact_session(session_id: String) -> Result<SessionData, String> {
    storage::compact_session(&session_id).await
}

const MAX_AGENTS_MD_BYTES: usize = 1024 * 1024;

fn workspace_agents_path(cwd: &str) -> Result<std::path::PathBuf, String> {
    if cwd.trim().is_empty() {
        return Err("No active workspace selected. Please open a workspace folder first.".into());
    }
    agent::tools::resolve_workspace_path(cwd, "AGENTS.md")
}

#[tauri::command]
async fn init_workspace_agents_md(state: State<'_, AppState>) -> Result<String, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    let path = workspace_agents_path(&cwd)?;
    if path.exists() {
        return Ok(format!("AGENTS.md already exists at {}", path.display()));
    }

    let is_tauri = std::path::Path::new(&cwd)
        .join("src-tauri")
        .join("tauri.conf.json")
        .exists();

    let is_rust = std::path::Path::new(&cwd).join("Cargo.toml").exists() || is_tauri;
    let is_node = std::path::Path::new(&cwd).join("package.json").exists();
    let is_python = std::path::Path::new(&cwd).join("pyproject.toml").exists()
        || std::path::Path::new(&cwd).join("requirements.txt").exists();
    let is_go = std::path::Path::new(&cwd).join("go.mod").exists();

    let stack_desc = if is_tauri && is_node {
        "Tauri desktop app (Rust backend + TypeScript/Node frontend)"
    } else if is_rust && is_node {
        "Full-stack Rust + TypeScript/Node project"
    } else if is_rust {
        "Rust project"
    } else if is_node {
        "TypeScript / JavaScript Node.js project"
    } else if is_python {
        "Python project"
    } else if is_go {
        "Go project"
    } else {
        "Software project"
    };

    let commands = if is_tauri && is_node {
        "- Build frontend: `npm run build`\n- Run dev: `npm run tauri dev`\n- Build release: `npm run tauri build`\n- Run tests: `cargo test --manifest-path src-tauri/Cargo.toml`"
    } else if is_node && is_rust {
        "- Build frontend: `npm run build`\n- Run frontend tests: `npm test`\n- Run Rust tests: `cargo test`"
    } else if is_node {
        "- Install dependencies: `npm install`\n- Build: `npm run build`\n- Test: `npm test`"
    } else if is_rust {
        "- Build: `cargo build`\n- Test: `cargo test`\n- Check: `cargo check`"
    } else if is_python {
        "- Run tests: `pytest`\n- Lint: `ruff check`"
    } else if is_go {
        "- Build: `go build ./...`\n- Test: `go test ./...`"
    } else {
        "- Refer to repository README for build and test workflows."
    };

    let content = format!(
        "# Project Guidelines for AetherAgent\n\n\
        ## Stack & Architecture\n\
        - Type: {}\n\n\
        ## Operating Principles\n\
        - Inspect relevant codebase files before modifying code.\n\
        - Maintain a clean, minimalist developer aesthetic.\n\
        - Never introduce emojis, emotes, or unnecessary conversational filler.\n\
        - Verify code builds and passes tests before concluding tasks.\n\n\
        ## Common Commands\n\
        {}\n\n\
        ## Quality Standards\n\
        - Write type-safe, self-documenting code.\n\
        - Preserve existing architecture and conventions.\n",
        stack_desc, commands
    );

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .map_err(|error| {
            format!("Failed to create AGENTS.md without overwriting existing data: {error}")
        })?;
    file.write_all(content.as_bytes())
        .await
        .map_err(|error| format!("Failed to write AGENTS.md: {error}"))?;
    file.sync_all()
        .await
        .map_err(|error| format!("Failed to flush AGENTS.md: {error}"))?;

    Ok(format!("Successfully created AGENTS.md in {}", cwd))
}

#[tauri::command]
async fn read_workspace_agents_md(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    if cwd.is_empty() {
        return Ok(None);
    }
    let path = workspace_agents_path(&cwd)?;
    if path.exists() {
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|error| format!("Could not inspect AGENTS.md: {error}"))?;
        if !metadata.is_file() || metadata.len() > MAX_AGENTS_MD_BYTES as u64 {
            return Err("AGENTS.md must be a regular file no larger than 1 MiB.".into());
        }
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
}

#[tauri::command]
async fn save_workspace_agents_md(
    state: State<'_, AppState>,
    content: String,
) -> Result<(), String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    if cwd.is_empty() {
        return Err("No active workspace selected.".into());
    }
    if content.len() > MAX_AGENTS_MD_BYTES {
        return Err("AGENTS.md exceeds the 1 MiB limit.".into());
    }
    let path = workspace_agents_path(&cwd)?;
    storage::atomic_write(&path, content.as_bytes()).await
}

#[tauri::command]
async fn fetch_provider_models(provider: ProviderConfig) -> Result<Vec<String>, String> {
    agent::provider::fetch_models(&provider).await
}

#[tauri::command]
fn get_workspace_cwd(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone())
}

#[tauri::command]
async fn set_workspace_cwd(state: State<'_, AppState>, new_cwd: String) -> Result<String, String> {
    let trimmed = new_cwd.trim();
    if trimmed.is_empty() {
        storage::clear_workspace().await?;
        *state
            .workspace_cwd
            .lock()
            .map_err(|_| "Workspace lock is unavailable".to_string())? = String::new();
        return Ok(String::new());
    }
    let canonical = std::fs::canonicalize(trimmed)
        .map_err(|error| format!("Workspace is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err("Workspace must be an existing directory".into());
    }
    let cwd = canonical.to_string_lossy().to_string();
    storage::save_workspace(&cwd).await?;
    *state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())? = cwd.clone();
    Ok(cwd)
}

#[tauri::command]
async fn list_workspace_files(
    state: State<'_, AppState>,
    query: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<String>, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    if cwd.is_empty() {
        return Ok(Vec::new());
    }
    agent::tools::list_workspace_files(&cwd, query.as_deref(), limit.unwrap_or(50)).await
}

#[tauri::command]
async fn list_skills(
    state: State<'_, AppState>,
    paths: Option<Vec<String>>,
) -> Result<Vec<SkillInfo>, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable")?
        .clone();
    let paths = agent::skills::workspace_skill_paths(&cwd, paths.as_deref().unwrap_or_default());
    Ok(agent::skills::load_skills(&paths).await)
}

#[tauri::command]
async fn save_skill(
    name: String,
    description: String,
    instructions: String,
) -> Result<SkillInfo, String> {
    agent::skills::save_skill_markdown(&name, &description, &instructions, true).await
}

#[tauri::command]
fn list_native_tools() -> serde_json::Value {
    agent::tools::get_agent_tools()
}

#[tauri::command]
async fn list_mcp_tools(
    state: State<'_, AppState>,
    server: McpServerConfig,
) -> Result<Vec<serde_json::Value>, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::mcp::list_server_tools(&server, &cwd).await
}

#[tauri::command]
async fn call_mcp_tool(
    state: State<'_, AppState>,
    server: McpServerConfig,
    name: String,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::mcp::call_server_tool(&server, &cwd, &name, arguments).await
}

#[tauri::command]
async fn git_status(state: State<'_, AppState>) -> Result<agent::git::GitRepoStatus, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::check_git_status(&cwd).await
}

#[tauri::command]
async fn git_commit(state: State<'_, AppState>, message: String) -> Result<String, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_commit(&cwd, &message).await
}

#[tauri::command]
async fn git_push(state: State<'_, AppState>) -> Result<String, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_push(&cwd).await
}

#[tauri::command]
async fn revert_file_changes(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_revert_file(&cwd, &path).await
}

#[tauri::command]
async fn stage_file_changes(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_stage_file(&cwd, &path).await
}

#[tauri::command]
async fn run_shell_command(
    state: State<'_, AppState>,
    command: String,
) -> Result<agent::pty::ShellCommandResult, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::pty::run_shell_command(&cwd, &command).await
}

#[tauri::command]
fn inspect_paths(paths: Vec<String>) -> Vec<agent::types::AttachedContext> {
    storage::inspect_paths(paths)
}

#[tauri::command]
async fn save_pasted_attachment(
    state: State<'_, AppState>,
    name: String,
    bytes: Vec<u8>,
) -> Result<agent::types::AttachedContext, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    storage::save_attachment(Some(&cwd), &name, &bytes).await
}

fn decode_percent(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let input_bytes = input.as_bytes();
    let mut i = 0;
    while i < input_bytes.len() {
        if input_bytes[i] == b'%' && i + 2 < input_bytes.len() {
            if let Ok(hex_val) = u8::from_str_radix(
                std::str::from_utf8(&input_bytes[i + 1..=i + 2]).unwrap_or(""),
                16,
            ) {
                bytes.push(hex_val);
                i += 3;
                continue;
            }
        }
        bytes.push(input_bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).to_string()
}

pub fn resolve_open_target(target: &str, workspace_cwd: &str) -> std::path::PathBuf {
    let raw = target.trim();
    let without_hash = raw.split('#').next().unwrap_or(raw);
    let without_query = without_hash.split('?').next().unwrap_or(without_hash);

    let clean = if let Some(stripped) = without_query.strip_prefix("file:///") {
        stripped
    } else if let Some(stripped) = without_query.strip_prefix("file://localhost/") {
        stripped
    } else if let Some(stripped) = without_query.strip_prefix("file://") {
        stripped
    } else {
        without_query
    };

    let decoded = decode_percent(clean);
    #[cfg(target_os = "windows")]
    let decoded = {
        let trimmed = if decoded.starts_with('/') && decoded.len() > 2 && decoded.chars().nth(2) == Some(':') {
            &decoded[1..]
        } else if decoded.starts_with('\\') && decoded.len() > 2 && decoded.chars().nth(2) == Some(':') {
            &decoded[1..]
        } else {
            &decoded
        };
        trimmed.trim_matches('"').replace('/', "\\")
    };
    #[cfg(not(target_os = "windows"))]
    let decoded = decoded;

    let p = std::path::PathBuf::from(&decoded);
    #[cfg(target_os = "windows")]
    {
        if p.is_absolute() {
            p
        } else if !workspace_cwd.trim().is_empty() {
            let win_cwd = workspace_cwd.replace('/', "\\");
            std::path::Path::new(&win_cwd).join(&p)
        } else if let Ok(current_dir) = std::env::current_dir() {
            current_dir.join(&p)
        } else {
            p
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if p.is_absolute() {
            p
        } else if !workspace_cwd.trim().is_empty() {
            std::path::Path::new(workspace_cwd).join(&p)
        } else if let Ok(current_dir) = std::env::current_dir() {
            current_dir.join(&p)
        } else {
            p
        }
    }
}

#[tauri::command]
async fn open_path_or_url(state: State<'_, AppState>, path_or_url: String) -> Result<(), String> {
    let target = path_or_url.trim();
    if target.is_empty() {
        return Ok(());
    }

    // 1. Web URLs: open in default browser
    if target.starts_with("http://") || target.starts_with("https://") || target.starts_with("mailto:") {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("rundll32")
                .creation_flags(0x08000000)
                .args(["url.dll,FileProtocolHandler", target])
                .spawn()
                .map_err(|e| format!("Failed to open URL in browser: {e}"))?;
            return Ok(());
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("open")
                .arg(target)
                .spawn()
                .map_err(|e| format!("Failed to open URL: {e}"))?;
            return Ok(());
        }
    }

    // 2. Local files / directories
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();

    let resolved = resolve_open_target(target, &cwd);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let path_str = resolved.to_string_lossy().to_string();
        std::process::Command::new("cmd")
            .creation_flags(0x08000000)
            .args(["/C", "start", "", &path_str])
            .spawn()
            .map_err(|e| format!("Failed to open file: {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open file: {e}"))?;
        Ok(())
    }
}

#[tauri::command]
async fn reveal_in_explorer(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let target = path.trim();
    if target.is_empty() {
        return Ok(());
    }
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    let resolved = resolve_open_target(target, &cwd);

    #[cfg(target_os = "windows")]
    {
        let win_path = resolved.to_string_lossy().replace('/', "\\");
        let mut cmd = std::process::Command::new("explorer");
        if resolved.is_dir() {
            cmd.arg(&win_path);
            cmd.current_dir(&resolved);
        } else if resolved.exists() {
            cmd.arg(format!("/select,{}", win_path));
            if let Some(parent) = resolved.parent() {
                if parent.exists() {
                    cmd.current_dir(parent);
                }
            }
        } else if let Some(parent) = resolved.parent() {
            let parent_path = parent.to_string_lossy().replace('/', "\\");
            if parent.exists() {
                cmd.arg(&parent_path);
                cmd.current_dir(parent);
            } else if !cwd.trim().is_empty() && std::path::Path::new(&cwd).exists() {
                let win_cwd = cwd.replace('/', "\\");
                cmd.arg(&win_cwd);
                cmd.current_dir(&cwd);
            } else {
                cmd.arg(format!("/select,{}", win_path));
            }
        } else if !cwd.trim().is_empty() && std::path::Path::new(&cwd).exists() {
            let win_cwd = cwd.replace('/', "\\");
            cmd.arg(&win_cwd);
            cmd.current_dir(&cwd);
        } else {
            cmd.arg(format!("/select,{}", win_path));
        }
        cmd.spawn()
            .map_err(|e| format!("Failed to reveal in explorer: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &resolved.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("Failed to reveal in finder: {e}"))?;
        Ok(())
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let parent = resolved.parent().unwrap_or(&resolved);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("Failed to open directory: {e}"))?;
        Ok(())
    }
}

#[tauri::command]
async fn open_terminal(state: State<'_, AppState>, path: Option<String>) -> Result<(), String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    let target_dir = if let Some(p) = path {
        let p_trimmed = p.trim();
        if !p_trimmed.is_empty() {
            let resolved = resolve_open_target(p_trimmed, &cwd);
            if resolved.is_dir() {
                resolved
            } else {
                resolved.parent().unwrap_or(&resolved).to_path_buf()
            }
        } else if !cwd.is_empty() {
            std::path::PathBuf::from(&cwd)
        } else {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        }
    } else if !cwd.is_empty() {
        std::path::PathBuf::from(&cwd)
    } else {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    };

    let valid_dir = if target_dir.exists() && target_dir.is_dir() {
        target_dir
    } else if let Some(parent) = target_dir.parent() {
        if parent.exists() && parent.is_dir() {
            parent.to_path_buf()
        } else if !cwd.is_empty() && std::path::Path::new(&cwd).exists() {
            std::path::PathBuf::from(&cwd)
        } else {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        }
    } else if !cwd.is_empty() && std::path::Path::new(&cwd).exists() {
        std::path::PathBuf::from(&cwd)
    } else {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    };

    #[cfg(target_os = "windows")]
    {
        let dir_str = valid_dir.to_string_lossy().replace('/', "\\");
        std::process::Command::new("cmd")
            .args(["/C", "start", "powershell", "-NoExit", "-Command", &format!("Set-Location -LiteralPath '{}'", dir_str.replace('\'', "''"))])
            .spawn()
            .map_err(|e| format!("Failed to open terminal: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-a", "Terminal", &target_dir.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("Failed to open terminal: {e}"))?;
        Ok(())
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::process::Command::new("x-terminal-emulator")
            .current_dir(&target_dir)
            .spawn()
            .or_else(|_| {
                std::process::Command::new("terminal")
                    .current_dir(&target_dir)
                    .spawn()
            })
            .map_err(|e| format!("Failed to open terminal: {e}"))?;
        Ok(())
    }
}

#[tauri::command]
async fn list_background_tasks() -> Result<Vec<agent::tasks::TaskInfo>, String> {
    Ok(agent::tasks::get_task_manager().list_tasks().await)
}

#[tauri::command]
async fn kill_background_task(task_id: String) -> Result<String, String> {
    agent::tasks::get_task_manager().kill_task(&task_id).await
}

#[tauri::command]
async fn get_background_task_output(task_id: String, tail_lines: Option<usize>) -> Result<agent::tasks::TaskInfo, String> {
    agent::tasks::get_task_manager().get_status(&task_id, tail_lines).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_cwd = storage::load_workspace_sync();
    agent::skills::init_skills_environment();
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                            let _ = win.emit("toggle-spotlight", ());
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            if let Ok(shortcut) = "Alt+Space".parse::<Shortcut>() {
                let _ = app.global_shortcut().register(shortcut);
            }
            Ok(())
        })
        .manage(AppState {
            workspace_cwd: Mutex::new(initial_cwd),
            runner: Arc::new(AgentRunner::new()),
        })
        .invoke_handler(tauri::generate_handler![
            send_message,
            review_session,
            read_reflection_skill,
            send_council_message,
            abort_agent,
            get_settings,
            save_settings,
            list_sessions,
            load_session,
            save_session,
            rename_session,
            toggle_pin_session,
            list_all_artifacts,
            deep_search,
            delete_session,
            export_session,
            fetch_provider_models,
            get_workspace_cwd,
            set_workspace_cwd,
            list_workspace_files,
            list_skills,
            save_skill,
            list_native_tools,
            list_mcp_tools,
            call_mcp_tool,
            git_status,
            git_commit,
            git_push,
            revert_file_changes,
            stage_file_changes,
            run_shell_command,
            compact_session,
            init_workspace_agents_md,
            read_workspace_agents_md,
            save_workspace_agents_md,
            inspect_paths,
            save_pasted_attachment,
            open_path_or_url,
            reveal_in_explorer,
            open_terminal,
            list_background_tasks,
            kill_background_task,
            get_background_task_output
        ])
        .run(tauri::generate_context!())
        .expect("error while running AetherAgent application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_percent_escapes() {
        assert_eq!(decode_percent("hello%20world"), "hello world");
        assert_eq!(decode_percent("D:/My%20Documents/Tugas%201/file.docx"), "D:/My Documents/Tugas 1/file.docx");
        assert_eq!(decode_percent("plain_text.txt"), "plain_text.txt");
    }

    #[test]
    fn test_resolve_open_target_decodes_and_strips_file_scheme() {
        #[cfg(target_os = "windows")]
        {
            let resolved = resolve_open_target("file:///D:/folder%20name/doc.docx", "");
            assert_eq!(resolved.to_string_lossy(), "D:\\folder name\\doc.docx");

            let resolved2 = resolve_open_target("D:/UNIKOM/file.docx", "");
            assert_eq!(resolved2.to_string_lossy(), "D:\\UNIKOM\\file.docx");

            let resolved3 = resolve_open_target("file:///D:/folder%20name/doc.docx#L12-L20", "");
            assert_eq!(resolved3.to_string_lossy(), "D:\\folder name\\doc.docx");

            let resolved4 = resolve_open_target("file:///D:/folder with space/doc.docx", "");
            assert_eq!(resolved4.to_string_lossy(), "D:\\folder with space\\doc.docx");

            let resolved5 = resolve_open_target("clutter/script.py", "D:/UNIKOM/Pemrograman");
            assert_eq!(resolved5.to_string_lossy(), "D:\\UNIKOM\\Pemrograman\\clutter\\script.py");
        }
    }
}
