use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::io::AsyncWriteExt;

mod agent;
use agent::events::{AgentHost, EventSink};
use agent::runner::AgentRunner;
use agent::storage;
use agent::types::{
    AgentRunRequest, AgentSettings, Artifact, McpServerConfig, ProviderConfig, Session,
    SessionData, SessionPage, SessionWindow, SkillInfo,
};

/// Desktop-only adapter; the shared backend never depends on Tauri or a WebView.
struct TauriEventSink(AppHandle);

impl EventSink for TauriEventSink {
    fn emit(&self, topic: &str, payload: serde_json::Value) -> Result<(), String> {
        self.0.emit(topic, payload).map_err(|error| error.to_string())
    }
}

impl From<AppHandle> for AgentHost {
    fn from(app: AppHandle) -> Self {
        Self::new(TauriEventSink(app))
    }
}

fn host(app: AppHandle) -> AgentHost {
    app.into()
}

pub struct AppState {
    pub workspace_cwd: Mutex<String>,
    pub runner: Arc<AgentRunner>,
}

fn bind_workspace(explicit: Option<&str>, fallback: &str) -> Result<String, String> {
    storage::canonical_workspace(explicit.unwrap_or(fallback))
}

fn validate_memory_workspace(expected: Option<&str>, current: &str) -> Result<(), String> {
    if let Some(expected) = expected {
        let expected = storage::canonical_workspace(expected)?;
        let current = storage::canonical_workspace(current)?;
        if expected != current {
            return Err(
                "Workspace changed while the memory editor was open; reopen the editor".into(),
            );
        }
    }
    Ok(())
}

fn validate_council_scope(a: &AgentRunRequest, b: &AgentRunRequest) -> Result<(), String> {
    if a.run_id != b.run_id || a.session_id != b.session_id || a.execution_mode != b.execution_mode
    {
        return Err("Council candidates must share run, session, and execution scope".into());
    }
    Ok(())
}

#[tauri::command]
async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    mut request: AgentRunRequest,
) -> Result<(), String> {
    storage::hydrate_run_history(&mut request).await?;
    agent::images::validate_request_images(&request.messages, &request.provider)?;
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    let runner = state.runner.clone();
    let cwd = bind_workspace(request.workspace_cwd.as_deref(), &cwd)?;
    let cancel = runner.begin(&request.run_id, &request.session_id)?;
    let run_id = request.run_id.clone();
    tokio::spawn(async move {
        runner.run(host(app), cwd, request, cancel).await;
        runner.finish(&run_id);
    });
    Ok(())
}

#[tauri::command]
async fn send_council_message(
    app: AppHandle,
    state: State<'_, AppState>,
    mut request_a: AgentRunRequest,
    mut request_b: AgentRunRequest,
) -> Result<(), String> {
    validate_council_scope(&request_a, &request_b)?;
    storage::hydrate_run_history(&mut request_a).await?;
    storage::hydrate_run_history(&mut request_b).await?;
    agent::images::validate_request_images(&request_a.messages, &request_a.provider)?;
    agent::images::validate_request_images(&request_b.messages, &request_b.provider)?;
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    let runner = state.runner.clone();
    validate_council_scope(&request_a, &request_b)?;
    let cwd_a = bind_workspace(request_a.workspace_cwd.as_deref(), &cwd)?;
    let cwd_b = bind_workspace(request_b.workspace_cwd.as_deref(), &cwd)?;
    if cwd_a != cwd_b {
        return Err("Council candidates must share the same workspace".into());
    }
    let cwd = cwd_a;
    let cancel = runner.begin(&request_a.run_id, &request_a.session_id)?;
    let run_id = request_a.run_id.clone();
    tokio::spawn(async move {
        runner
            .run_council(host(app), cwd, request_a, request_b, cancel)
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
    state.runner.review_session(host(app), session_id, provider).await
}

// Keep this in sync with review::creation_root; relative configuration is home-,
// not workspace-relative. The review module owns its implementation.
fn review_creation_root(config: &agent::types::ReviewConfig) -> Result<std::path::PathBuf, String> {
    agent::skills::review_creation_root(config)
}

fn linked_review_change<'a>(
    data: &'a SessionData,
    skill_name: &str,
    run_id: Option<&str>,
    file_path: Option<&str>,
) -> Result<
    (
        &'a agent::types::SelfImprovementEvent,
        Option<&'a agent::types::SelfImprovementChange>,
    ),
    String,
> {
    for review in data
        .messages
        .iter()
        .rev()
        .filter_map(|m| m.self_improvement.as_ref())
    {
        if review.session_id != data.session.id || run_id.is_some_and(|id| id != review.run_id) {
            continue;
        }
        let changes: Vec<_> = review
            .changes
            .iter()
            .filter(|change| {
                if change.skill_name != skill_name {
                    return false;
                }
                file_path.is_none_or(|requested| {
                    change.file_path.as_deref() == Some(requested)
                        || change
                            .skill_path
                            .as_ref()
                            .zip(change.file_path.as_ref())
                            .is_some_and(|(root, target)| {
                                !std::path::Path::new(requested).is_absolute()
                                    && std::path::Path::new(root).join(requested)
                                        == std::path::Path::new(target)
                            })
                })
            })
            .collect();
        if changes.len() > 1 && changes.iter().any(|c| c.file_path != changes[0].file_path) {
            return Err("Multiple reviewed files match this skill; select a file_path".into());
        }
        if let Some(change) = changes.first() {
            return Ok((review, Some(*change)));
        }
        // Only genuinely legacy events authorize the workspace-generated fallback.
        if review.changes.is_empty()
            && review.skill_name.as_deref() == Some(skill_name)
            && file_path.is_none()
        {
            return Ok((review, None));
        }
    }
    Err("This skill file is not associated with a review in the selected session".into())
}

#[tauri::command]
async fn read_reflection_skill(
    session_id: String,
    skill_name: String,
    run_id: Option<String>,
    file_path: Option<String>,
) -> Result<String, String> {
    let data = storage::load_session(&session_id).await?;
    let (review, change) =
        linked_review_change(&data, &skill_name, run_id.as_deref(), file_path.as_deref())?;
    let cwd = review
        .workspace_cwd
        .as_ref()
        .or(data.session.workspace_cwd.as_ref());
    let Some(change) = change else {
        return agent::reflection::read_generated_skill(
            cwd.ok_or("The session has no workspace")?,
            &skill_name,
        )
        .await;
    };
    let settings = storage::load_settings().await?;
    let mut store = agent::review_skills::ReviewSkillStore::new(
        cwd.map(String::as_str).unwrap_or(""),
        &settings.skill_paths,
        review_creation_root(&settings.review_config)?,
        agent::review_pending::pending_root()?,
        settings.review_config.write_approval,
        &review.run_id,
        &session_id,
    )
    .await?;
    let target = change
        .file_path
        .as_deref()
        .ok_or("The reviewed change has no linked file")?;
    let root = change
        .skill_path
        .as_deref()
        .ok_or("The reviewed change has no skill directory")?;
    let relative = std::path::Path::new(target)
        .strip_prefix(root)
        .map_err(|_| "The reviewed file is outside its skill directory")?;
    let relative = relative
        .to_str()
        .ok_or("Invalid reviewed file path")?
        .replace('\\', "/");
    let view = store
        .execute(
            "skill_view",
            &serde_json::json!({"name":skill_name,"file_path":relative}),
        )
        .await?;
    // Name collisions or changed catalog precedence must never show another file.
    if view["file_path"].as_str().map(std::path::Path::new) != Some(std::path::Path::new(target)) {
        return Err("The discovered skill does not match the reviewed file".into());
    }
    view["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "Skill content unavailable".into())
}

#[tauri::command]
async fn list_review_proposals(session_id: String) -> Result<Vec<serde_json::Value>, String> {
    storage::load_session(&session_id).await?;
    agent::review_pending::list_pending(&agent::review_pending::pending_root()?, &session_id).await
}

#[tauri::command]
async fn review_proposal_diff(session_id: String, id: String) -> Result<serde_json::Value, String> {
    agent::review_pending::pending_diff(&agent::review_pending::pending_root()?, &id, &session_id)
        .await
}

fn validate_pending_session(
    data: &SessionData,
    session_id: &str,
    run_id: &str,
    id: &str,
) -> Result<(), String> {
    if data.session.id != session_id || run_id.is_empty() {
        return Err("Pending proposal session/run mismatch".into());
    }
    let linked = data
        .messages
        .iter()
        .filter(|m| m.role == "assistant")
        .filter_map(|m| m.self_improvement.as_ref())
        .any(|e| {
            e.session_id == session_id
                && e.run_id == run_id
                && e.changes
                    .iter()
                    .any(|c| c.pending_id.as_deref() == Some(id))
        });
    if linked
        || data.messages.iter().any(|m| {
            m.role == "assistant"
                && (m.run_id.as_deref() == Some(run_id) || m.id == run_id)
                && m.self_improvement
                    .as_ref()
                    .is_none_or(|e| e.session_id == session_id && e.run_id == run_id)
        })
    {
        return Ok(());
    }
    Err("The originating assistant turn is not saved; wait for the run to finish and retry".into())
}

/// Select the entire authoritative event, not a synthetic single-change update.
fn resolved_review_event<'a>(
    data: &'a SessionData,
    session_id: &str,
    run_id: &str,
    id: &str,
) -> Result<&'a agent::types::SelfImprovementEvent, String> {
    use agent::types::SelfImprovementAction;
    if data.session.id == session_id {
        if let Some(event) = data
            .messages
            .iter()
            .filter(|m| m.role == "assistant")
            .filter_map(|m| m.self_improvement.as_ref())
            .find(|e| {
                e.session_id == session_id
                    && e.run_id == run_id
                    && e.changes.iter().any(|c| {
                        c.pending_id.as_deref() == Some(id)
                            && c.action != SelfImprovementAction::SkillStaged
                    })
            })
        {
            return Ok(event);
        }
    }
    Err("Resolved proposal event is unavailable in native history".into())
}

async fn emit_resolved_review(
    app: &AppHandle,
    session_id: &str,
    run_id: &str,
    id: &str,
) -> Result<(), String> {
    let data = storage::load_session(session_id).await?;
    app.emit(
        "agent:self-improvement",
        resolved_review_event(&data, session_id, run_id, id)?,
    )
    .map_err(|e| e.to_string())
}

async fn validate_review_proposal(session_id: &str, id: &str) -> Result<String, String> {
    let root = agent::review_pending::pending_root()?;
    let proposal = agent::review_pending::pending_diff(&root, id, session_id).await?;
    let run_id = proposal["run_id"]
        .as_str()
        .ok_or("Pending proposal has no run")?;
    let data = storage::load_session(session_id).await?;
    validate_pending_session(&data, session_id, run_id, id)?;
    // Revalidate against sealed staging authority and merge under SESSION_IO_LOCK
    // before either decision removes the native pending record.
    storage::attach_pending_link(&root, id, session_id).await?;
    Ok(run_id.to_owned())
}

#[tauri::command]
async fn approve_review_proposal(
    app: AppHandle,
    session_id: String,
    id: String,
) -> Result<agent::types::SelfImprovementChange, String> {
    let run_id = validate_review_proposal(&session_id, &id).await?;
    let mut change = agent::review_pending::approve_pending(&agent::review_pending::pending_root()?, &id, &session_id)
        .await.map_err(|error| format!("Approval did not complete: {error}. A skill write may already have occurred if pending cleanup failed; inspect the file before retrying."))?;
    change.pending_id = Some(id.clone());
    // Do not report an applied mutation as an unapplied failure.
    let refresh = async {
        storage::resolve_review_proposal(&session_id, &id, Some(change.clone())).await?;
        emit_resolved_review(&app, &session_id, &run_id, &id).await
    }
    .await;
    if let Err(error) = refresh {
        change.summary = Some(format!("{}; WARNING: skill operation completed, but review history could not be saved or refreshed: {error}", change.summary.as_deref().unwrap_or("Approved review change")));
    }
    Ok(change)
}

#[tauri::command]
async fn reject_review_proposal(
    app: AppHandle,
    session_id: String,
    id: String,
) -> Result<(), String> {
    let run_id = validate_review_proposal(&session_id, &id).await?;
    agent::review_pending::reject_pending(
        &agent::review_pending::pending_root()?,
        &id,
        &session_id,
    )
    .await?;
    let refresh = async {
        storage::resolve_review_proposal(&session_id, &id, None).await?;
        emit_resolved_review(&app, &session_id, &run_id, &id).await
    }
    .await;
    refresh.map_err(|error| {
        format!("Proposal rejected, but review history could not be saved or refreshed: {error}")
    })
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
async fn warm_session_index(
    limit: usize,
    budget_ms: u64,
) -> Result<agent::session_pages::WarmIndexResult, String> {
    agent::history_maintenance::warm_session_index(limit, budget_ms).await
}

#[tauri::command]
async fn list_sessions() -> Result<Vec<Session>, String> {
    storage::list_sessions().await
}

#[tauri::command]
async fn load_session(session_id: String) -> Result<SessionData, String> {
    storage::load_session(&session_id).await
}

#[tauri::command]
async fn load_session_page(
    session_id: String,
    before: Option<usize>,
    limit: usize,
    revision: Option<String>,
) -> Result<SessionPage, String> {
    storage::load_session_page(&session_id, before, limit, revision).await
}

#[tauri::command]
async fn load_session_around(
    session_id: String,
    message_id: String,
    limit: usize,
) -> Result<SessionPage, String> {
    storage::load_session_around(&session_id, &message_id, limit).await
}

#[tauri::command]
async fn save_session_page(
    data: SessionData,
    window: SessionWindow,
) -> Result<SessionWindow, String> {
    storage::save_session_page(data, window).await
}

#[tauri::command]
async fn recover_save_ack(
    session_id: String,
    proof: storage::CommittedSaveProof,
) -> Result<SessionPage, String> {
    storage::recover_save_ack(&session_id, proof).await
}

#[tauri::command]
async fn refresh_session_window(
    session_id: String,
    data: SessionData,
    window: SessionWindow,
    allow_metadata: bool,
    review_run_id: Option<String>,
) -> Result<SessionWindow, String> {
    storage::refresh_session_window(
        &session_id,
        data,
        window,
        allow_metadata,
        review_run_id.as_deref(),
    )
    .await
}
#[tauri::command]
async fn checkpoint_session_page(
    data: SessionData,
    window: SessionWindow,
) -> Result<SessionWindow, String> {
    storage::checkpoint_session_page(data, window).await
}

#[tauri::command]
async fn read_message_image_inputs(
    session_id: String,
    message_id: String,
) -> Result<Vec<agent::types::ImageInput>, String> {
    storage::read_message_image_inputs(&session_id, &message_id).await
}

#[tauri::command]
async fn read_session_artifact(
    session_id: String,
    artifact_id: String,
) -> Result<Artifact, String> {
    storage::read_session_artifact(&session_id, &artifact_id).await
}

#[tauri::command]
async fn rename_session_summary(session_id: String, title: String) -> Result<Session, String> {
    storage::rename_session_summary(&session_id, &title).await
}

#[tauri::command]
async fn generate_session_title(
    session_id: String,
    provider: agent::types::ProviderConfig,
    prompt: String,
    response_snippet: Option<String>,
) -> Result<String, String> {
    let title = agent::runner::generate_title_with_provider(&provider, &prompt, response_snippet.as_deref()).await?;
    let cleaned = agent::runner::clean_generated_title(&title);
    if cleaned.is_empty() {
        return Err("Generated title was empty".into());
    }
    let renamed = storage::rename_session_summary(&session_id, &cleaned).await?;
    Ok(renamed.title)
}
#[tauri::command]
async fn save_session(data: SessionData) -> Result<(), String> {
    storage::save_session(data).await
}

#[tauri::command]
async fn checkpoint_session_delta(
    session_id: String,
    request: agent::types::CheckpointDelta,
) -> Result<agent::types::CheckpointAck, String> {
    storage::checkpoint_session_delta(&session_id, request).await
}

#[tauri::command]
async fn checkpoint_session(data: SessionData) -> Result<(), String> {
    storage::checkpoint_session(data).await
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
    storage::list_all_artifacts().await
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
    agent::tasks::get_task_manager().kill_session_tasks(&session_id).await;
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
async fn init_workspace_agents_md(
    state: State<'_, AppState>,
    expected_workspace: Option<String>,
) -> Result<String, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    validate_memory_workspace(expected_workspace.as_deref(), &cwd)?;
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
async fn read_workspace_agents_md(
    state: State<'_, AppState>,
    expected_workspace: Option<String>,
) -> Result<Option<String>, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    if cwd.is_empty() {
        return Ok(None);
    }
    validate_memory_workspace(expected_workspace.as_deref(), &cwd)?;
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
    expected_workspace: Option<String>,
) -> Result<(), String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    if cwd.is_empty() {
        return Err("No active workspace selected.".into());
    }
    validate_memory_workspace(expected_workspace.as_deref(), &cwd)?;
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
    #[cfg(target_os = "windows")]
    let cwd = {
        let s = canonical.to_string_lossy().to_string();
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    };
    #[cfg(not(target_os = "windows"))]
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
    expected_workspace: Option<String>,
) -> Result<Vec<SkillInfo>, String> {
    let fallback = state.workspace_cwd.lock().map_err(|_| "Workspace lock is unavailable")?.clone();
    let cwd = bind_workspace(expected_workspace.as_deref(), &fallback)?;
    let settings = storage::load_settings().await?;
    let paths = agent::skills::effective_skill_paths(
        &cwd,
        paths.as_deref().unwrap_or(&settings.skill_paths),
        &settings.review_config,
    )?;
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
async fn git_fetch(state: State<'_, AppState>) -> Result<String, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_fetch(&cwd).await
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
async fn prepare_image_attachments(
    attachments: Vec<agent::types::AttachedContext>,
) -> Result<Vec<agent::types::ImageInput>, String> {
    tokio::task::spawn_blocking(move || agent::images::prepare_attachments(&attachments))
        .await
        .map_err(|e| format!("Image preparation task failed: {e}"))?
}

#[tauri::command]
async fn save_pasted_attachment(
    state: State<'_, AppState>,
    name: String,
    bytes: Vec<u8>,
    expected_workspace: Option<String>,
) -> Result<agent::types::AttachedContext, String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    // Reject a workspace switch during the frontend file read, then bind the
    // destination before the asynchronous write (never re-read global state).
    validate_memory_workspace(expected_workspace.as_deref(), &cwd)?;
    let cwd = bind_workspace(expected_workspace.as_deref(), &cwd)?;
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
        let trimmed = if (decoded.starts_with('/') || decoded.starts_with('\\'))
            && decoded.len() > 2
            && decoded.chars().nth(2) == Some(':')
        {
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

// Build the exact native-open argument; no command interpreter is involved.
fn build_open_target(target: &str, cwd: &str) -> Result<std::ffi::OsString, String> {
    let target = target.trim();
    if target.is_empty() || target.chars().any(char::is_control) {
        return Err("Invalid open target".into());
    }
    if let Ok(url) = url::Url::parse(target) {
        if matches!(url.scheme(), "http" | "https" | "mailto") {
            if url.scheme() != "mailto" && url.host_str().is_none() {
                return Err("URL requires a host".into());
            }
            return Ok(target.into());
        }
        let windows_drive = target.as_bytes().get(1) == Some(&b':')
            && target
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic);
        if url.scheme() != "file" && !windows_drive {
            return Err("Unsupported URL scheme".into());
        }
        if url.scheme() == "file"
            && url
                .host_str()
                .is_some_and(|host| !host.is_empty() && host != "localhost")
        {
            return Err("Remote file targets are not allowed".into());
        }
    }
    let resolved = resolve_open_target(target, cwd);
    let resolved_text = resolved.to_string_lossy();
    let clean = resolved_text
        .strip_prefix(r"\\?\")
        .unwrap_or(&resolved_text);
    if clean.starts_with(r"\\") || clean.starts_with("//") {
        return Err("UNC/device targets are not allowed".into());
    }
    let canonical = std::fs::canonicalize(&resolved)
        .map_err(|error| format!("Open target is unavailable: {error}"))?;
    let metadata = std::fs::metadata(&canonical).map_err(|e| e.to_string())?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err("Target must be a regular file or directory".into());
    }
    if metadata.is_file() {
        let extension = canonical
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        // Permit documents only; arbitrary registered extensions can launch code.
        if !matches!(
            extension.as_str(),
            "txt"
                | "md"
                | "markdown"
                | "pdf"
                | "json"
                | "csv"
                | "log"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "bmp"
                | "svg"
                | "html"
                | "htm"
                | "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
                | "odt"
                | "ods"
                | "rtf"
        ) {
            return Err(
                "Opening executable, shortcut, or unknown file types is not allowed".into(),
            );
        }
    }
    Ok(canonical.into_os_string())
}

#[cfg(target_os = "windows")]
fn native_open(target: &std::ffi::OsStr) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            window: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    let file: Vec<u16> = target.encode_wide().chain(std::iter::once(0)).collect();
    if file[..file.len() - 1].contains(&0) {
        return Err("Open target contains NUL".into());
    }
    let operation: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    // File and verb are passed directly to ShellExecute, with no parameters and no cmd.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result <= 32 {
        Err(format!("Native opener failed (Windows error {result})"))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn native_open(target: &std::ffi::OsStr) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Native opener failed: {error}"))
}

#[tauri::command]
async fn open_path_or_url(state: State<'_, AppState>, path_or_url: String) -> Result<(), String> {
    let cwd = state
        .workspace_cwd
        .lock()
        .map_err(|_| "Workspace lock is unavailable")?
        .clone();
    let target = build_open_target(&path_or_url, &cwd)?;
    native_open(&target)
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
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell.exe")
            .creation_flags(0x00000010)
            .current_dir(&valid_dir)
            .args(["-NoLogo", "-NoExit"])
            .spawn()
            .map_err(|e| format!("Failed to open terminal: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-a", "Terminal", &valid_dir.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("Failed to open terminal: {e}"))?;
        Ok(())
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::process::Command::new("x-terminal-emulator")
            .current_dir(&valid_dir)
            .spawn()
            .or_else(|_| {
                std::process::Command::new("terminal")
                    .current_dir(&valid_dir)
                    .spawn()
            })
            .map_err(|e| format!("Failed to open terminal: {e}"))?;
        Ok(())
    }
}

#[tauri::command]
async fn list_background_tasks(session_id: String) -> Result<Vec<agent::tasks::TaskInfo>, String> {
    Ok(agent::tasks::get_task_manager()
        .list_tasks(&session_id)
        .await)
}

#[tauri::command]
async fn kill_background_task(session_id: String, task_id: String) -> Result<String, String> {
    agent::tasks::get_task_manager()
        .kill_task(&session_id, &task_id)
        .await
}

#[tauri::command]
async fn get_background_task_output(
    session_id: String,
    task_id: String,
    tail_lines: Option<usize>,
) -> Result<agent::tasks::TaskInfo, String> {
    agent::tasks::get_task_manager()
        .get_status(&session_id, &task_id, tail_lines)
        .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(feature = "diagnostics")]
    if let Err(error) = agent::native_smoke::enter() {
        eprintln!("Diagnostic mode rejected: {error}");
        std::process::exit(2);
    }
    #[cfg(not(feature = "diagnostics"))]
    if std::env::args().skip(1).any(|arg| {
        arg.starts_with("--diagnostic") || arg.starts_with("--smoke-test") || arg == "--data-dir"
    }) {
        eprintln!("Diagnostic flags require a build with --features diagnostics");
        std::process::exit(2);
    }
    let context = tauri::generate_context!();
    #[cfg(feature = "diagnostics")]
    let mut context = context;
    #[cfg(feature = "diagnostics")]
    if let Some(root) = agent::native_smoke::profile() {
        for window in &mut context.config_mut().app.windows {
            window.data_directory = Some(root.join("webview-profile"));
            window.title = "AetherAgent ? isolated diagnostic profile".into();
        }
    }
    let initial_cwd = storage::load_workspace_sync();
    agent::skills::init_skills_environment();
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(agent::release::ReleaseState::default())
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
            #[cfg(feature = "diagnostics")]
            if agent::native_smoke::profile().is_some() {
                return Ok(());
            }
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
            agent::release::get_release_info,
            agent::release::check_release_update,
            agent::release::install_release_update,
            agent::release::restart_after_update,
            send_message,
            review_session,
            read_reflection_skill,
            list_review_proposals,
            review_proposal_diff,
            approve_review_proposal,
            reject_review_proposal,
            send_council_message,
            abort_agent,
            get_settings,
            save_settings,
            list_sessions,
            warm_session_index,
            load_session,
            load_session_page,
            load_session_around,
            save_session_page,
            recover_save_ack,
            refresh_session_window,
            checkpoint_session_page,
            read_session_artifact,
            read_message_image_inputs,
            rename_session_summary,
            generate_session_title,
            save_session,
            checkpoint_session,
            checkpoint_session_delta,
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
            git_fetch,
            revert_file_changes,
            stage_file_changes,
            run_shell_command,
            compact_session,
            init_workspace_agents_md,
            read_workspace_agents_md,
            save_workspace_agents_md,
            inspect_paths,
            save_pasted_attachment,
            prepare_image_attachments,
            open_path_or_url,
            reveal_in_explorer,
            open_terminal,
            list_background_tasks,
            kill_background_task,
            get_background_task_output
        ])
        .build(context)
        .expect("error while building AetherAgent application")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                agent::tasks::get_task_manager().cancel_all();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_preflight_precedes_run_registration_and_runner_side_effects() {
        let ipc = include_str!("lib.rs");
        let single = ipc
            .split("async fn send_message(")
            .nth(1)
            .unwrap()
            .split("#[tauri::command]")
            .next()
            .unwrap();
        let council = ipc
            .split("async fn send_council_message(")
            .nth(1)
            .unwrap()
            .split("#[tauri::command]")
            .next()
            .unwrap();
        let validation = "validate_request_images(&request.messages, &request.provider)?";
        assert!(
            single
                .find(validation)
                .is_some_and(|i| i < single.find("runner.begin(").unwrap()),
            "single IPC must validate before begin"
        );
        for candidate in ["request_a", "request_b"] {
            let validation =
                format!("validate_request_images(&{candidate}.messages, &{candidate}.provider)?");
            assert!(
                council
                    .find(&validation)
                    .is_some_and(|i| i < council.find("runner.begin(").unwrap()),
                "both council candidates must validate before begin"
            );
        }
        let runner = include_str!("agent/runner.rs");
        let single = runner
            .split("pub async fn run(")
            .nth(1)
            .unwrap()
            .split("let mcp_servers")
            .next()
            .unwrap();
        let council = runner
            .split("pub async fn run_council(")
            .nth(1)
            .unwrap()
            .split("let default_prompt")
            .next()
            .unwrap();
        assert!(single.contains("validate_request_images(&request.messages, &request.provider)"));
        for candidate in ["request_a", "request_b"] {
            assert!(council.contains(&format!(
                "validate_request_images(&{candidate}.messages, &{candidate}.provider)"
            )));
        }
        let council_history = runner
            .split("pub async fn run_council(")
            .nth(1)
            .unwrap()
            .split("let empty_tools_a")
            .next()
            .unwrap();
        assert!(council_history.contains("append_history_message(&mut conv_a, message)"));
        assert!(council_history.contains("append_history_message(&mut conv_b, message)"));
        for entry in [single, council] {
            assert!(entry.contains("emitter.error(error)"));
            assert!(entry.contains("emitter.done(false, false)"));
        }
        let message: agent::types::ChatMessage = serde_json::from_value(serde_json::json!({
            "id":"image", "role":"user", "content":"describe", "timestamp":1,
            "imageInputs":[{"sourcePath":"missing.png", "mediaType":"image/png", "data":"iVBORw0KGgo="}]
        })).unwrap();
        for model in ["deepseek-chat", "deepseek-reasoner"] {
            let provider = ProviderConfig {
                model: model.into(),
                ..Default::default()
            };
            let error =
                agent::images::validate_request_images(std::slice::from_ref(&message), &provider)
                    .unwrap_err();
            assert!(
                error.contains(model)
                    && error.contains("text-only")
                    && error.contains("vision-capable")
            );
        }
        let mut malformed = message;
        malformed.image_inputs.as_mut().unwrap()[0].data = "not base64".into();
        assert!(
            agent::images::validate_request_images(&[malformed], &ProviderConfig::default())
                .is_err()
        );
    }

    #[test]
    fn review_proposal_event_selection_and_pending_run_validation_are_scoped() {
        let mut data: SessionData = serde_json::from_value(serde_json::json!({
            "session":{"id":"session","title":"Test","createdAt":1,"updatedAt":1,"messageCount":1},
            "messages":[{"id":"message","runId":"run","role":"assistant","content":"done","timestamp":1}],"artifacts":[]
        })).unwrap();
        assert!(validate_pending_session(&data, "session", "run", "pending").is_ok());
        assert!(
            validate_pending_session(&data, "session", "unsaved", "pending")
                .unwrap_err()
                .contains("wait")
        );
        assert!(validate_pending_session(&data, "foreign", "run", "pending").is_err());
        data.messages[0].role = "user".into();
        assert!(validate_pending_session(&data, "session", "run", "pending").is_err());
        data.messages[0].role = "assistant".into();
        data.messages[0].self_improvement = Some(serde_json::from_value(serde_json::json!({
            "run_id":"run","session_id":"session","action":"skill_updated","summary":"keep summary","workspace_cwd":"workspace",
            "changes":[{"action":"skill_updated","skill_name":"one","pending_id":"pending"},
                       {"action":"skill_staged","skill_name":"two","pending_id":"other"}]
        })).unwrap());
        let event = resolved_review_event(&data, "session", "run", "pending").unwrap();
        assert_eq!(Some(event), data.messages[0].self_improvement.as_ref());
        assert_eq!(event.changes.len(), 2);
        assert!(resolved_review_event(&data, "session", "run", "other").is_err());
        assert!(resolved_review_event(&data, "foreign", "run", "pending").is_err());
        assert!(resolved_review_event(&data, "session", "foreign", "pending").is_err());
        assert!(resolved_review_event(&data, "session", "run", "missing").is_err());
        data.messages[0].self_improvement.as_mut().unwrap().changes[0].action =
            agent::types::SelfImprovementAction::NoAction;
        assert!(resolved_review_event(&data, "session", "run", "pending").is_ok());
    }
    #[test]
    fn review_link_lookup_is_session_run_and_exact_file_scoped() {
        let data: SessionData = serde_json::from_value(serde_json::json!({
            "session":{"id":"session","title":"Test","createdAt":1,"updatedAt":1,"messageCount":1},
            "messages":[{"id":"message","role":"assistant","content":"reviewed","timestamp":1,
                "selfImprovement":{"run_id":"run","session_id":"session","action":"skill_updated","skill_name":"legacy",
                    "changes":[
                        {"action":"skill_updated","skill_name":"manual","skill_path":"/skills/manual","file_path":"/skills/manual/SKILL.md"},
                        {"action":"skill_created","skill_name":"manual","skill_path":"/skills/manual","file_path":"/skills/manual/references/notes.md"}
                    ]}}],"artifacts":[]
        })).unwrap();
        assert!(linked_review_change(&data, "manual", Some("run"), None).is_err());
        let (_, change) =
            linked_review_change(&data, "manual", Some("run"), Some("references/notes.md"))
                .unwrap();
        assert_eq!(
            change.unwrap().file_path.as_deref(),
            Some("/skills/manual/references/notes.md")
        );
        assert!(linked_review_change(
            &data,
            "manual",
            Some("foreign"),
            Some("references/notes.md")
        )
        .is_err());
        assert!(linked_review_change(&data, "manual", None, Some("/outside/secret")).is_err());
        assert!(linked_review_change(&data, "legacy", None, None).is_err());
        let mut foreign = data.clone();
        foreign.session.id = "foreign".into();
        assert!(
            linked_review_change(&foreign, "manual", None, Some("references/notes.md")).is_err()
        );
        let mut legacy = data;
        legacy.messages[0]
            .self_improvement
            .as_mut()
            .unwrap()
            .changes
            .clear();
        assert!(linked_review_change(&legacy, "legacy", None, None)
            .unwrap()
            .1
            .is_none());
        assert!(linked_review_change(&legacy, "legacy", None, Some("SKILL.md")).is_err());
    }

    #[test]
    fn memory_workspace_binding_rejects_stale_editor_without_fallback() {
        let current = std::env::current_dir().unwrap();
        let current = current.to_string_lossy();
        assert!(validate_memory_workspace(Some(""), &current).is_err());
        assert!(validate_memory_workspace(Some("missing-editor-workspace"), &current).is_err());
        assert!(validate_memory_workspace(Some(&current), &current).is_ok());
        assert!(validate_memory_workspace(None, &current).is_ok());
    }

    #[test]
    fn regression_workspace_binding_is_explicit_and_fail_closed() {
        assert_eq!(bind_workspace(Some(""), "missing-global").unwrap(), "");
        assert!(bind_workspace(Some("missing-explicit"), "").is_err());
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            bind_workspace(None, &cwd.to_string_lossy()).unwrap(),
            storage::canonical_workspace(&cwd.to_string_lossy()).unwrap()
        );
    }

    #[test]
    fn regression_council_scope_rejected_before_registry_begin() {
        let a: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "runId":"run-a", "sessionId":"session-a", "provider":{}, "messages":[]
        }))
        .unwrap();
        let mut b = a.clone();
        assert!(validate_council_scope(&a, &b).is_ok());
        b.run_id = "different-run".into();
        assert!(validate_council_scope(&a, &b).is_err());
        b = a.clone();
        b.session_id = "different-session".into();
        assert!(validate_council_scope(&a, &b).is_err());
        b = a.clone();
        b.execution_mode = agent::types::ExecutionMode::RequestReview;
        assert!(validate_council_scope(&a, &b).is_err());
    }

    #[test]
    fn regression_native_open_builder_keeps_metacharacters_literal() {
        let root = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("aether-open-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("literal & % ^ spaces.txt");
        std::fs::write(&file, "fixture").unwrap();
        let target = build_open_target(
            "literal%20%26%20%25%20%5E%20spaces.txt",
            &root.to_string_lossy(),
        )
        .unwrap();
        assert_eq!(
            target,
            std::fs::canonicalize(&file).unwrap().into_os_string()
        );
        for unsafe_target in [
            "javascript:alert(1)",
            "custom:payload",
            "missing.txt",
            "a\0b",
            r"\\server\share\file.txt",
        ] {
            assert!(build_open_target(unsafe_target, &root.to_string_lossy()).is_err());
        }
        for ext in ["exe", "cmd", "bat", "lnk", "url", "ps1", "js", "vbs"] {
            let executable = root.join(format!("blocked.{ext}"));
            std::fs::write(&executable, "fixture").unwrap();
            assert!(build_open_target(&executable.to_string_lossy(), "").is_err());
        }
        assert_eq!(
            build_open_target("https://example.com/a?q=%26", "").unwrap(),
            std::ffi::OsString::from("https://example.com/a?q=%26")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_decode_percent_escapes() {
        assert_eq!(decode_percent("hello%20world"), "hello world");
        assert_eq!(
            decode_percent("D:/My%20Documents/Tugas%201/file.docx"),
            "D:/My Documents/Tugas 1/file.docx"
        );
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
            assert_eq!(
                resolved4.to_string_lossy(),
                "D:\\folder with space\\doc.docx"
            );

            let resolved5 = resolve_open_target("clutter/script.py", "D:/UNIKOM/Pemrograman");
            assert_eq!(
                resolved5.to_string_lossy(),
                "D:\\UNIKOM\\Pemrograman\\clutter\\script.py"
            );
        }
    }
}
