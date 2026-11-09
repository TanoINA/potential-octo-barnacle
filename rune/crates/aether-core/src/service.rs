//! Renderer-neutral native command facade. Argument keys accept the desktop's
//! camelCase spelling and Rust snake_case; nested objects retain their schemas.
use crate::agent;
use agent::types::{
    AgentRunRequest, AgentSettings, Artifact, McpServerConfig, ProviderConfig, Session,
    SessionData, SessionPage, SessionWindow, SkillInfo,
};
use agent::{events::AgentHost, runner::AgentRunner, storage};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

pub struct Backend {
    pub runner: Arc<AgentRunner>,
    pub workspace: Arc<Mutex<String>>,
    pub host: AgentHost,
    pub updater: Option<Arc<crate::updater::Updater>>,
}

impl Backend {
    /// Reads only the saved workspace; does not initialize settings or discover
    /// skills/tasks. The host must configure its profile before constructing this.
    pub fn new(host: AgentHost) -> Self {
        Self {
            runner: Arc::new(AgentRunner::new()),
            workspace: Arc::new(Mutex::new(storage::load_workspace_sync())),
            host,
            updater: crate::updater::Updater::compiled().map(Arc::new),
        }
    }

    pub async fn dispatch(&self, command: &str, args: Value) -> Result<Value, String> {
        if !args.is_object() {
            return Err("Command arguments must be a JSON object".into());
        }
        match command {
            "send_message" => value(send_message(self.host.clone(), self, argument(&args, "request")?).await?),
            "send_council_message" => value(send_council_message(self.host.clone(), self, argument(&args, "requestA")?, argument(&args, "requestB")?).await?),
            "review_session" => value(review_session(self.host.clone(), self, argument(&args, "sessionId")?, argument(&args, "provider")?).await?),
            "read_reflection_skill" => value(read_reflection_skill(argument(&args, "sessionId")?, argument(&args, "skillName")?, optional_argument(&args, "runId")?, optional_argument(&args, "filePath")?).await?),
            "list_review_proposals" => value(list_review_proposals(argument(&args, "sessionId")?).await?),
            "review_proposal_diff" => value(review_proposal_diff(argument(&args, "sessionId")?, argument(&args, "id")?).await?),
            "approve_review_proposal" => value(approve_review_proposal(self.host.clone(), argument(&args, "sessionId")?, argument(&args, "id")?).await?),
            "reject_review_proposal" => value(reject_review_proposal(self.host.clone(), argument(&args, "sessionId")?, argument(&args, "id")?).await?),
            "abort_agent" => value(abort_agent(self, argument(&args, "runId")?)?),
            "get_settings" => value(get_settings().await?),
            "save_settings" => value(save_settings(argument(&args, "settings")?).await?),
            "warm_session_index" => value(warm_session_index(argument(&args, "limit")?, argument(&args, "budgetMs")?).await?),
            "list_sessions" => value(list_sessions().await?),
            "load_session" => value(load_session(argument(&args, "sessionId")?).await?),
            "load_session_page" => value(load_session_page(argument(&args, "sessionId")?, optional_argument(&args, "before")?, argument(&args, "limit")?, optional_argument(&args, "revision")?).await?),
            "load_session_around" => value(load_session_around(argument(&args, "sessionId")?, argument(&args, "messageId")?, argument(&args, "limit")?).await?),
            "save_session_page" => value(save_session_page(argument(&args, "data")?, argument(&args, "window")?).await?),
            "recover_save_ack" => value(storage::recover_save_ack(&argument::<String>(&args, "sessionId")?, argument(&args, "proof")?).await?),
            "replace_session_checked" => value(storage::replace_session_checked(argument(&args, "data")?, argument(&args, "expectedWindow")?).await?),
            "refresh_session_window" => value(refresh_session_window(argument(&args, "sessionId")?, argument(&args, "data")?, argument(&args, "window")?, argument(&args, "allowMetadata")?, optional_argument(&args, "reviewRunId")?).await?),
            "checkpoint_session_page" => value(checkpoint_session_page(argument(&args, "data")?, argument(&args, "window")?).await?),
            "read_message_image_inputs" => value(read_message_image_inputs(argument(&args, "sessionId")?, argument(&args, "messageId")?).await?),
            "read_session_artifact" => value(read_session_artifact(argument(&args, "sessionId")?, argument(&args, "artifactId")?).await?),
            "rename_session_summary" => value(rename_session_summary(argument(&args, "sessionId")?, argument(&args, "title")?).await?),
            "save_session" => value(save_session(argument(&args, "data")?).await?),
            "checkpoint_session_delta" => value(checkpoint_session_delta(argument(&args, "sessionId")?, argument(&args, "request")?).await?),
            "checkpoint_session" => value(checkpoint_session(argument(&args, "data")?).await?),
            "rename_session" => value(rename_session(argument(&args, "sessionId")?, argument(&args, "title")?).await?),
            "toggle_pin_session" => value(toggle_pin_session(argument(&args, "sessionId")?).await?),
            "list_all_artifacts" => value(list_all_artifacts().await?),
            "deep_search" => value(deep_search(argument(&args, "query")?, optional_argument(&args, "limit")?).await?),
            "delete_session" => value(delete_session(argument(&args, "sessionId")?).await?),
            "export_session" => value(export_session(self, argument(&args, "sessionId")?, argument(&args, "destination")?, argument(&args, "format")?).await?),
            "compact_session" => value(compact_session(argument(&args, "sessionId")?).await?),
            "init_workspace_agents_md" => value(init_workspace_agents_md(self, optional_argument(&args, "expectedWorkspace")?).await?),
            "read_workspace_agents_md" => value(read_workspace_agents_md(self, optional_argument(&args, "expectedWorkspace")?).await?),
            "save_workspace_agents_md" => value(save_workspace_agents_md(self, argument(&args, "content")?, optional_argument(&args, "expectedWorkspace")?).await?),
            "fetch_provider_models" => value(fetch_provider_models(argument(&args, "provider")?).await?),
            "get_workspace_cwd" => value(get_workspace_cwd(self)?),
            "set_workspace_cwd" => value(set_workspace_cwd(self, argument(&args, "newCwd")?).await?),
            "list_workspace_files" => value(list_workspace_files(self, optional_argument(&args, "query")?, optional_argument(&args, "limit")?).await?),
            "list_skills" => value(list_skills(self, optional_argument(&args, "paths")?).await?),
            "save_skill" => value(save_skill(argument(&args, "name")?, argument(&args, "description")?, argument(&args, "instructions")?).await?),
            "list_native_tools" => value(list_native_tools()),
            "list_mcp_tools" => value(list_mcp_tools(self, argument(&args, "server")?).await?),
            "call_mcp_tool" => value(call_mcp_tool(self, argument(&args, "server")?, argument(&args, "name")?, argument(&args, "arguments")?).await?),
            "git_status" => value(git_status(self).await?),
            "git_commit" => value(git_commit(self, argument(&args, "message")?).await?),
            "git_push" => value(git_push(self).await?),
            "git_fetch" => value(git_fetch(self).await?),
            "revert_file_changes" => value(revert_file_changes(self, argument(&args, "path")?).await?),
            "stage_file_changes" => value(stage_file_changes(self, argument(&args, "path")?).await?),
            "run_shell_command" => value(run_shell_command(self, argument(&args, "command")?).await?),
            "inspect_paths" => value(inspect_paths(argument(&args, "paths")?)),
            "prepare_image_attachments" => value(prepare_image_attachments(argument(&args, "attachments")?).await?),
            "save_pasted_attachment" => value(save_pasted_attachment(self, argument(&args, "name")?, argument(&args, "bytes")?, optional_argument(&args, "expectedWorkspace")?).await?),
            "open_path_or_url" => value(open_path_or_url(self, argument(&args, "pathOrUrl")?).await?),
            "reveal_in_explorer" => value(reveal_in_explorer(self, argument(&args, "path")?).await?),
            "open_terminal" => value(open_terminal(self, optional_argument(&args, "path")?).await?),
            "list_background_tasks" => value(list_background_tasks(argument(&args, "sessionId")?).await?),
            "kill_background_task" => value(kill_background_task(argument(&args, "sessionId")?, argument(&args, "taskId")?).await?),
            "get_background_task_output" => value(get_background_task_output(argument(&args, "sessionId")?, argument(&args, "taskId")?, optional_argument(&args, "tailLines")?).await?),
            "get_release_info" => release_info(self.updater.as_deref()),
            "check_release_update" => {
                release_arguments(&args, &[])?;
                match &self.updater {
                    Some(updater) => value(updater.check().await?),
                    None => value(crate::updater::Updater::disabled()),
                }
            }
            "install_release_update" => {
                release_arguments(&args, &["expectedVersion", "expected_version", "ticketId", "ticket_id", "confirmed"])?;
                let updater = self.updater.as_ref().ok_or("Automatic updates not configured for this Rune build")?;
                value(updater.install(&argument::<String>(&args, "expectedVersion")?, &argument::<String>(&args, "ticketId")?, argument(&args, "confirmed")?).await?)
            }
            _ => Err(format!("Unknown command: {command}")),
        }
    }

    pub fn supported_commands() -> &'static [&'static str] {
        supported_commands()
    }
}

fn value<T: Serialize>(result: T) -> Result<Value, String> {
    serde_json::to_value(result).map_err(|error| error.to_string())
}

fn argument_value<'a>(args: &'a Value, key: &str) -> Result<Option<&'a Value>, String> {
    let mut snake = String::new();
    for ch in key.chars() {
        if ch.is_ascii_uppercase() {
            snake.push('_');
            snake.push(ch.to_ascii_lowercase());
        } else {
            snake.push(ch);
        }
    }
    let primary = args.get(key);
    let alternate = if snake != key { args.get(&snake) } else { None };
    if primary.is_some() && alternate.is_some() {
        return Err(format!("Ambiguous argument: {key} and {snake}"));
    }
    Ok(primary.or(alternate))
}

fn argument<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    let input =
        argument_value(args, key)?.ok_or_else(|| format!("Missing required argument: {key}"))?;
    serde_json::from_value(input.clone())
        .map_err(|error| format!("Invalid argument {key}: {error}"))
}

fn optional_argument<T: DeserializeOwned>(args: &Value, key: &str) -> Result<Option<T>, String> {
    match argument_value(args, key)? {
        None | Some(Value::Null) => Ok(None),
        Some(input) => serde_json::from_value(input.clone())
            .map(Some)
            .map_err(|error| format!("Invalid argument {key}: {error}")),
    }
}

fn release_arguments(args: &Value, permitted: &[&str]) -> Result<(), String> {
    if args.as_object().is_some_and(|args| args.keys().any(|key| !permitted.contains(&key.as_str()))) {
        return Err("Unexpected update argument; URLs, keys and installer paths are build-owned".into());
    }
    Ok(())
}
fn release_info(updater: Option<&crate::updater::Updater>) -> Result<Value, String> {
    let config = updater.map(crate::updater::Updater::status).unwrap_or_else(|| Err("Automatic updates not configured for this Rune build".into()));
    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "buildId": option_env!("AETHER_BUILD_ID").unwrap_or("unavailable"),
        "platform": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "diagnosticLogsPath": storage::config_dir().join("tasks").to_string_lossy(),
        "signingStatus": "Unknown (not verified at runtime)",
        "updatesConfigured": config.is_ok(),
        "updateStatus": config.err().unwrap_or_else(|| "Signed updates configured; availability has not been checked".into()),
        "updateAction": if cfg!(target_os = "windows") { "install" } else { "stage" },
    }))
}
/// Commands implemented by the renderer-neutral host (no installer placeholder).
pub fn supported_commands() -> &'static [&'static str] {
    &[
        "send_message",
        "send_council_message",
        "review_session",
        "read_reflection_skill",
        "list_review_proposals",
        "review_proposal_diff",
        "approve_review_proposal",
        "reject_review_proposal",
        "abort_agent",
        "get_settings",
        "save_settings",
        "warm_session_index",
        "list_sessions",
        "load_session",
        "load_session_page",
        "load_session_around",
        "save_session_page",
        "recover_save_ack",
        "replace_session_checked",
        "refresh_session_window",
        "checkpoint_session_page",
        "read_message_image_inputs",
        "read_session_artifact",
        "rename_session_summary",
        "save_session",
        "checkpoint_session_delta",
        "checkpoint_session",
        "rename_session",
        "toggle_pin_session",
        "list_all_artifacts",
        "deep_search",
        "delete_session",
        "export_session",
        "compact_session",
        "init_workspace_agents_md",
        "read_workspace_agents_md",
        "save_workspace_agents_md",
        "fetch_provider_models",
        "get_workspace_cwd",
        "set_workspace_cwd",
        "list_workspace_files",
        "list_skills",
        "save_skill",
        "list_native_tools",
        "list_mcp_tools",
        "call_mcp_tool",
        "git_status",
        "git_commit",
        "git_push",
        "git_fetch",
        "revert_file_changes",
        "stage_file_changes",
        "run_shell_command",
        "inspect_paths",
        "prepare_image_attachments",
        "save_pasted_attachment",
        "open_path_or_url",
        "reveal_in_explorer",
        "open_terminal",
        "list_background_tasks",
        "kill_background_task",
        "get_background_task_output",
        "get_release_info",
        "check_release_update",
        "install_release_update",
    ]
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

async fn send_message(
    app: AgentHost,
    state: &Backend,
    mut request: AgentRunRequest,
) -> Result<(), String> {
    storage::hydrate_run_history(&mut request).await?;
    agent::images::validate_request_images(&request.messages, &request.provider)?;
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    let runner = state.runner.clone();
    let cwd = bind_workspace(request.workspace_cwd.as_deref(), &cwd)?;
    let cancel = runner.begin(&request.run_id, &request.session_id)?;
    let run_id = request.run_id.clone();
    tokio::spawn(async move {
        runner.run(app, cwd, request, cancel).await;
        runner.finish(&run_id);
    });
    Ok(())
}

async fn send_council_message(
    app: AgentHost,
    state: &Backend,
    mut request_a: AgentRunRequest,
    mut request_b: AgentRunRequest,
) -> Result<(), String> {
    validate_council_scope(&request_a, &request_b)?;
    storage::hydrate_run_history(&mut request_a).await?;
    storage::hydrate_run_history(&mut request_b).await?;
    agent::images::validate_request_images(&request_a.messages, &request_a.provider)?;
    agent::images::validate_request_images(&request_b.messages, &request_b.provider)?;
    let cwd = state
        .workspace
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
            .run_council(app, cwd, request_a, request_b, cancel)
            .await;
        runner.finish(&run_id);
    });
    Ok(())
}

async fn review_session(
    app: AgentHost,
    state: &Backend,
    session_id: String,
    provider: ProviderConfig,
) -> Result<(), String> {
    state.runner.review_session(app, session_id, provider).await
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

async fn list_review_proposals(session_id: String) -> Result<Vec<serde_json::Value>, String> {
    storage::load_session(&session_id).await?;
    agent::review_pending::list_pending(&agent::review_pending::pending_root()?, &session_id).await
}

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
    app: &AgentHost,
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

async fn approve_review_proposal(
    app: AgentHost,
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

async fn reject_review_proposal(
    app: AgentHost,
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

fn abort_agent(state: &Backend, run_id: String) -> Result<(), String> {
    state.runner.abort(&run_id)
}

async fn get_settings() -> Result<AgentSettings, String> {
    storage::load_settings().await
}

async fn save_settings(settings: AgentSettings) -> Result<(), String> {
    storage::save_settings(&settings).await
}

async fn warm_session_index(
    limit: usize,
    budget_ms: u64,
) -> Result<agent::session_pages::WarmIndexResult, String> {
    agent::history_maintenance::warm_session_index(limit, budget_ms).await
}

async fn list_sessions() -> Result<Vec<Session>, String> {
    storage::list_sessions().await
}

async fn load_session(session_id: String) -> Result<SessionData, String> {
    storage::load_session(&session_id).await
}

async fn load_session_page(
    session_id: String,
    before: Option<usize>,
    limit: usize,
    revision: Option<String>,
) -> Result<SessionPage, String> {
    storage::load_session_page(&session_id, before, limit, revision).await
}

async fn load_session_around(
    session_id: String,
    message_id: String,
    limit: usize,
) -> Result<SessionPage, String> {
    storage::load_session_around(&session_id, &message_id, limit).await
}

async fn save_session_page(
    data: SessionData,
    window: SessionWindow,
) -> Result<SessionWindow, String> {
    storage::save_session_page(data, window).await
}

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

async fn checkpoint_session_page(
    data: SessionData,
    window: SessionWindow,
) -> Result<SessionWindow, String> {
    storage::checkpoint_session_page(data, window).await
}

async fn read_message_image_inputs(
    session_id: String,
    message_id: String,
) -> Result<Vec<agent::types::ImageInput>, String> {
    storage::read_message_image_inputs(&session_id, &message_id).await
}

async fn read_session_artifact(
    session_id: String,
    artifact_id: String,
) -> Result<Artifact, String> {
    storage::read_session_artifact(&session_id, &artifact_id).await
}

async fn rename_session_summary(session_id: String, title: String) -> Result<Session, String> {
    storage::rename_session_summary(&session_id, &title).await
}

async fn save_session(data: SessionData) -> Result<(), String> {
    storage::save_session(data).await
}

async fn checkpoint_session_delta(
    session_id: String,
    request: agent::types::CheckpointDelta,
) -> Result<agent::types::CheckpointAck, String> {
    storage::checkpoint_session_delta(&session_id, request).await
}

async fn checkpoint_session(data: SessionData) -> Result<(), String> {
    storage::checkpoint_session(data).await
}

async fn rename_session(session_id: String, title: String) -> Result<SessionData, String> {
    storage::rename_session(&session_id, &title).await
}

async fn toggle_pin_session(session_id: String) -> Result<Session, String> {
    storage::toggle_pin_session(&session_id).await
}

async fn list_all_artifacts() -> Result<Vec<agent::types::GlobalArtifactItem>, String> {
    storage::list_all_artifacts().await
}

async fn deep_search(
    query: String,
    limit: Option<usize>,
) -> Result<Vec<agent::search::DeepSearchResult>, String> {
    agent::search::search_query(&query, limit.unwrap_or(20))
}

async fn delete_session(session_id: String) -> Result<(), String> {
    storage::delete_session(&session_id).await
}

async fn export_session(
    state: &Backend,
    session_id: String,
    destination: String,
    format: String,
) -> Result<(), String> {
    let requested = std::path::PathBuf::from(&destination);
    let resolved = if requested.is_absolute() {
        requested
    } else {
        let cwd = state
            .workspace
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

async fn init_workspace_agents_md(
    state: &Backend,
    expected_workspace: Option<String>,
) -> Result<String, String> {
    let cwd = state
        .workspace
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

async fn read_workspace_agents_md(
    state: &Backend,
    expected_workspace: Option<String>,
) -> Result<Option<String>, String> {
    let cwd = state
        .workspace
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

async fn save_workspace_agents_md(
    state: &Backend,
    content: String,
    expected_workspace: Option<String>,
) -> Result<(), String> {
    let cwd = state
        .workspace
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

async fn fetch_provider_models(provider: ProviderConfig) -> Result<Vec<String>, String> {
    agent::provider::fetch_models(&provider).await
}

fn get_workspace_cwd(state: &Backend) -> Result<String, String> {
    Ok(state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone())
}

async fn set_workspace_cwd(state: &Backend, new_cwd: String) -> Result<String, String> {
    let trimmed = new_cwd.trim();
    if trimmed.is_empty() {
        storage::clear_workspace().await?;
        *state
            .workspace
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
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())? = cwd.clone();
    Ok(cwd)
}

async fn list_workspace_files(
    state: &Backend,
    query: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<String>, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    if cwd.is_empty() {
        return Ok(Vec::new());
    }
    agent::tools::list_workspace_files(&cwd, query.as_deref(), limit.unwrap_or(50)).await
}

async fn list_skills(
    state: &Backend,
    paths: Option<Vec<String>>,
) -> Result<Vec<SkillInfo>, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable")?
        .clone();
    let settings = storage::load_settings().await?;
    let paths = agent::skills::effective_skill_paths(
        &cwd,
        paths.as_deref().unwrap_or(&settings.skill_paths),
        &settings.review_config,
    )?;
    Ok(agent::skills::load_skills(&paths).await)
}

async fn save_skill(
    name: String,
    description: String,
    instructions: String,
) -> Result<SkillInfo, String> {
    agent::skills::save_skill_markdown(&name, &description, &instructions, true).await
}

fn list_native_tools() -> serde_json::Value {
    agent::tools::get_agent_tools()
}

async fn list_mcp_tools(
    state: &Backend,
    server: McpServerConfig,
) -> Result<Vec<serde_json::Value>, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::mcp::list_server_tools(&server, &cwd).await
}

async fn call_mcp_tool(
    state: &Backend,
    server: McpServerConfig,
    name: String,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::mcp::call_server_tool(&server, &cwd, &name, arguments).await
}

async fn git_status(state: &Backend) -> Result<agent::git::GitRepoStatus, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::check_git_status(&cwd).await
}

async fn git_commit(state: &Backend, message: String) -> Result<String, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_commit(&cwd, &message).await
}

async fn git_push(state: &Backend) -> Result<String, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_push(&cwd).await
}

async fn git_fetch(state: &Backend) -> Result<String, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_fetch(&cwd).await
}

async fn revert_file_changes(state: &Backend, path: String) -> Result<String, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_revert_file(&cwd, &path).await
}

async fn stage_file_changes(state: &Backend, path: String) -> Result<String, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::git::git_stage_file(&cwd, &path).await
}

async fn run_shell_command(
    state: &Backend,
    command: String,
) -> Result<agent::pty::ShellCommandResult, String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable".to_string())?
        .clone();
    agent::pty::run_shell_command(&cwd, &command).await
}

fn inspect_paths(paths: Vec<String>) -> Vec<agent::types::AttachedContext> {
    storage::inspect_paths(paths)
}

async fn prepare_image_attachments(
    attachments: Vec<agent::types::AttachedContext>,
) -> Result<Vec<agent::types::ImageInput>, String> {
    tokio::task::spawn_blocking(move || agent::images::prepare_attachments(&attachments))
        .await
        .map_err(|e| format!("Image preparation task failed: {e}"))?
}

async fn save_pasted_attachment(
    state: &Backend,
    name: String,
    bytes: Vec<u8>,
    expected_workspace: Option<String>,
) -> Result<agent::types::AttachedContext, String> {
    let cwd = state
        .workspace
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
    let decoded = if without_query.starts_with("file://") && !decoded.starts_with('/') {
        format!("/{decoded}")
    } else {
        decoded
    };

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
    if target.chars().any(char::is_control) {
        return Err("Invalid open target".into());
    }
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
    let child = std::process::Command::new(program)
        .arg(target)
        .spawn()
        .map_err(|error| format!("Native opener failed: {error}"))?;
    check_launcher(child, true)
}

async fn open_path_or_url(state: &Backend, path_or_url: String) -> Result<(), String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable")?
        .clone();
    let target = build_open_target(&path_or_url, &cwd)?;
    native_open(&target)
}

async fn reveal_in_explorer(state: &Backend, path: String) -> Result<(), String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable")?
        .clone();
    let resolved = local_native_path(&path, &cwd)?;
    #[cfg(target_os = "windows")]
    let child = {
        let mut cmd = std::process::Command::new("explorer.exe");
        if resolved.is_dir() {
            cmd.arg(&resolved);
        } else {
            cmd.arg(format!("/select,{}", resolved.display()));
        }
        cmd.spawn()
    };
    #[cfg(target_os = "macos")]
    let child = std::process::Command::new("open")
        .arg("-R")
        .arg(&resolved)
        .spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let child = std::process::Command::new("xdg-open")
        .arg(if resolved.is_dir() {
            resolved.as_path()
        } else {
            resolved.parent().ok_or("Target has no parent")?
        })
        .spawn();
    let child = child.map_err(|error| format!("Failed to reveal path: {error}"))?;
    tokio::task::spawn_blocking(move || check_launcher(child, true))
        .await
        .map_err(|e| e.to_string())?
}

async fn open_terminal(state: &Backend, path: Option<String>) -> Result<(), String> {
    let cwd = state
        .workspace
        .lock()
        .map_err(|_| "Workspace lock is unavailable")?
        .clone();
    let target = path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or(if cwd.is_empty() { "." } else { &cwd });
    let resolved = local_native_path(target, &cwd)?;
    let directory = if resolved.is_dir() {
        resolved.as_path()
    } else {
        resolved.parent().ok_or("Target has no directory")?
    };
    #[cfg(target_os = "windows")]
    let child = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell.exe")
            .creation_flags(0x00000010)
            .current_dir(directory)
            .args(["-NoLogo", "-NoExit"])
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let child = std::process::Command::new("open")
        .args(["-a", "Terminal"])
        .arg(directory)
        .spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let child = std::process::Command::new("x-terminal-emulator")
        .current_dir(directory)
        .spawn()
        .or_else(|_| {
            std::process::Command::new("terminal")
                .current_dir(directory)
                .spawn()
        });
    let child = child.map_err(|error| format!("Failed to open terminal: {error}"))?;
    tokio::task::spawn_blocking(move || check_launcher(child, cfg!(target_os = "macos")))
        .await
        .map_err(|e| e.to_string())?
}

// Local-only native actions reject URLs, network/device paths and controls.
fn local_native_path(target: &str, cwd: &str) -> Result<std::path::PathBuf, String> {
    if target.trim().is_empty() || target.chars().any(char::is_control) {
        return Err("Invalid native path".into());
    }
    if let Ok(url) = url::Url::parse(target.trim()) {
        let windows_drive = cfg!(windows)
            && target.as_bytes().get(1) == Some(&b':')
            && target.as_bytes()[0].is_ascii_alphabetic();
        if url.scheme() != "file" && !windows_drive {
            return Err("Native action requires a local path".into());
        }
        if url.scheme() == "file"
            && url
                .host_str()
                .is_some_and(|h| !h.is_empty() && h != "localhost")
        {
            return Err("Remote file targets are not allowed".into());
        }
    }
    let resolved = resolve_open_target(target, cwd);
    let text = resolved.to_string_lossy();
    let clean = text.strip_prefix(r"\\?\").unwrap_or(&text);
    if clean.starts_with(r"\\") || clean.starts_with("//") {
        return Err("UNC/device targets are not allowed".into());
    }
    let canonical = std::fs::canonicalize(&resolved)
        .map_err(|e| format!("Native target is unavailable: {e}"))?;
    let text = canonical.to_string_lossy();
    let clean = text.strip_prefix(r"\\?\").unwrap_or(&text);
    if clean.starts_with(r"\\") || clean.starts_with("//") {
        return Err("UNC/device targets are not allowed".into());
    }
    if !canonical.is_file() && !canonical.is_dir() {
        return Err("Target must be a regular file or directory".into());
    }
    Ok(canonical)
}

// Launchers must either exit successfully or explicitly report a bounded timeout.
// Interactive terminal processes are observed for startup failure, not awaited to
// user exit; a detached waiter reaps them on POSIX without blocking the renderer.
fn check_launcher(mut child: std::process::Child, wait_for_exit: bool) -> Result<(), String> {
    let deadline = std::time::Instant::now()
        + if wait_for_exit {
            std::time::Duration::from_secs(3)
        } else {
            std::time::Duration::from_millis(150)
        };
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("Native launcher status failed: {e}"))?
        {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("Native launcher exited with {status}"))
            };
        }
        if std::time::Instant::now() >= deadline {
            if wait_for_exit {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Native launcher did not finish within 3 seconds".into());
            }
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

async fn list_background_tasks(session_id: String) -> Result<Vec<agent::tasks::TaskInfo>, String> {
    Ok(agent::tasks::get_task_manager()
        .list_tasks(&session_id)
        .await)
}

async fn kill_background_task(session_id: String, task_id: String) -> Result<String, String> {
    agent::tasks::get_task_manager()
        .kill_task(&session_id, &task_id)
        .await
}

async fn get_background_task_output(
    session_id: String,
    task_id: String,
    tail_lines: Option<usize>,
) -> Result<agent::tasks::TaskInfo, String> {
    agent::tasks::get_task_manager()
        .get_status(&session_id, &task_id, tail_lines)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_review_proposal_event_selection_and_pending_run_validation_are_scoped() {
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
    fn service_review_link_lookup_is_session_run_and_exact_file_scoped() {
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
}
