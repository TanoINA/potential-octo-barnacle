use super::events::AgentHost;
use super::mcp::{result_text, McpManager};
use super::provider::{
    is_retryable_provider_error, stream_chat_completion, CompletionOptions, ToolDelta,
};
use super::skills::{
    effective_skill_paths, find_skill, load_skills, read_skill_content, render_skill_catalog,
};
use super::tools::{execute_tool, get_agent_tools, ToolContext, ToolEvent, ToolExecutionResult};
use super::types::{
    AgentEvent, AgentRunRequest, Artifact, ArtifactEventData, ChatMessage, CouncilChunkData,
    CouncilDoneData, DoneEventData, ErrorEventData, ExecutionMode, MicroCompactionEvent,
    NoticeEventData, ProviderConfig, ReviewConfig, SkillActivatedEventData, SkillInfo,
    StreamEventData, SubagentEventData, SubagentProgress, TerminalEventData, ToolCallItem,
    ToolEventData,
};
use futures_util::{stream, StreamExt};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SUBAGENT_MAX_TURNS: usize = 25;
const TOOL_BATCH_CONCURRENCY: usize = 4;
const LEAN_WORKFLOW: &str = "\n## Proportionate execution\nHandle small tasks directly without subagents. Delegate only substantial independent work or when explicitly requested; do not create automatic reviewer/fixer chains. Inspect partial work before replacing a failed subagent. Run focused tests while editing, then broader checks once after the final change when risk warrants them. Monitor a long-running task through its existing task handle; avoid repeated sleep-and-status shell commands. Preserve required security, validation and data-loss safeguards.\n";

async fn dispatch_tool_batch<I, F, Fut, C, T>(
    calls: I,
    cancel: &CancellationToken,
    dispatch: F,
    cancelled: C,
) -> Vec<T>
where
    I: IntoIterator,
    F: Fn(I::Item) -> Fut,
    Fut: std::future::Future<Output = T>,
    C: Fn(I::Item) -> T,
{
    // Batas berlaku per batch agar subagen tidak menunggu slot milik induknya.
    stream::iter(calls)
        .map(|call| {
            let (dispatch, cancelled) = (&dispatch, &cancelled);
            async move {
                // Periksa saat dipoll, sebelum closure dispatch dapat memicu efek samping.
                if cancel.is_cancelled() {
                    cancelled(call)
                } else {
                    dispatch(call).await
                }
            }
        })
        .buffered(TOOL_BATCH_CONCURRENCY)
        .collect()
        .await
}

struct PreparedCall {
    call_id: String,
    function_name: String,
    arguments_raw: String,
    ui_id: String,
    args: serde_json::Value,
    malformed_error: Option<String>,
}

fn cancelled_call(prep: PreparedCall, emitter: &EmitterContext) -> (String, ToolExecutionResult) {
    // Error terminal sudah diterbitkan saat persiapan; jangan timpa saat pembatalan.
    if let Some(err) = prep.malformed_error {
        return (
            prep.call_id,
            ToolExecutionResult {
                output: err,
                success: false,
                cancelled: false,
                ..Default::default()
            },
        );
    }
    emitter.tool(ToolCallItem {
        id: prep.ui_id,
        name: prep.function_name,
        args: prep.args,
        status: "cancelled".into(),
        result: Some("Cancelled by operator".into()),
        artifact_id: None,
        arguments_raw: Some(prep.arguments_raw),
        started_at: Some(chrono::Utc::now().timestamp_millis()),
        completed_at: Some(chrono::Utc::now().timestamp_millis()),
        duration_ms: Some(0),
        exit_code: None,
        terminal_output: None,
        files_changed: None,
    });
    (
        prep.call_id,
        ToolExecutionResult {
            output: "Cancelled by operator".into(),
            cancelled: true,
            ..Default::default()
        },
    )
}

#[derive(Clone)]
struct EmitterContext {
    app: AgentHost,
    run_id: String,
    session_id: String,
}

impl EmitterContext {
    fn emit(&self, event: AgentEvent) {
        let _ = self.app.emit("agent-event", event);
    }
    fn chunk(&self, text: String) {
        self.emit(AgentEvent::Chunk(StreamEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            text,
        }));
    }
    fn reasoning(&self, text: String) {
        self.emit(AgentEvent::Reasoning(StreamEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            text,
        }));
    }
    fn tool(&self, tool: ToolCallItem) {
        self.emit(AgentEvent::ToolUpdate(ToolEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            tool,
        }));
    }
    fn artifact(&self, artifact: Artifact) {
        self.emit(AgentEvent::Artifact(ArtifactEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            artifact,
        }));
    }
    fn terminal(&self, artifact_id: String, delta: String, stream: String) {
        self.emit(AgentEvent::TerminalOutput(TerminalEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            artifact_id,
            delta,
            stream,
        }));
    }
    fn error(&self, message: String) {
        self.emit(AgentEvent::Error(ErrorEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            message,
        }));
    }
    fn done(&self, cancelled: bool, successful: bool) {
        self.emit(AgentEvent::Done(DoneEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            cancelled,
            successful,
        }));
    }
    fn subagent(&self, subagent: SubagentProgress) {
        self.emit(AgentEvent::SubagentUpdate(SubagentEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            subagent,
        }));
    }
    fn skill(&self, skill: SkillInfo) {
        self.emit(AgentEvent::SkillActivated(SkillActivatedEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            skill,
        }));
    }
    fn notice(
        &self,
        message: String,
        attempt: Option<usize>,
        max_attempts: Option<usize>,
        reset_partial: bool,
    ) {
        self.emit(AgentEvent::Notice(NoticeEventData {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            message,
            attempt,
            max_attempts,
            reset_partial,
        }));
    }
    fn clear_notice(&self) {
        self.notice(String::new(), None, None, false);
    }
    fn provider_state(&self, messages: &[serde_json::Value]) {
        if valid_provider_messages(messages) {
            self.emit(AgentEvent::ProviderState(
                super::types::ProviderStateEventData {
                    run_id: self.run_id.clone(),
                    session_id: self.session_id.clone(),
                    provider_messages: messages.to_vec(),
                },
            ));
        }
    }
}

/// Per-run skill maintenance state. Clones share exact-file read authorization;
/// construction performs no filesystem discovery or directory creation.
#[derive(Clone)]
struct ForegroundSkillTools {
    store: Arc<tokio::sync::Mutex<Option<super::review_skills::ReviewSkillStore>>>,
    workspace: String,
    skill_paths: Vec<String>,
    config: ReviewConfig,
    run_id: String,
    session_id: String,
}

impl ForegroundSkillTools {
    fn new(
        workspace: &str,
        skill_paths: &[String],
        config: &ReviewConfig,
        run_id: &str,
        session_id: &str,
    ) -> Self {
        Self {
            store: Arc::new(tokio::sync::Mutex::new(None)),
            workspace: workspace.into(),
            skill_paths: skill_paths.to_vec(),
            config: config.clone(),
            run_id: run_id.into(),
            session_id: session_id.into(),
        }
    }

    /// Called only after foreground dispatch has finished; clones share the drain.
    async fn take_event(&self) -> Option<super::types::SelfImprovementEvent> {
        let mut guard = self.store.lock().await;
        let changes = guard.as_ref()?.changes();
        let first = changes.first()?;
        let event = super::types::SelfImprovementEvent {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            action: first.action.clone(),
            skill_name: Some(first.skill_name.clone()),
            summary: Some(format!("{} foreground skill change(s).", changes.len())),
            workspace_cwd: Some(self.workspace.clone()),
            changes,
        };
        guard.take();
        Some(event)
    }
    async fn execute(&self, name: &str, args: &serde_json::Value) -> ToolExecutionResult {
        let mapped;
        let (name, args) = if name == "learn_skill" {
            if args["overwrite"].as_bool() == Some(true) {
                return ToolExecutionResult::failure("Legacy learn_skill overwrite is not supported; read the exact file with skill_view, then use skill_manage patch/edit preserving frontmatter.".into());
            }
            mapped = serde_json::json!({
                "action": "create", "name": args["name"],
                "description": args["description"], "content": args["instructions"],
            });
            ("skill_manage", &mapped)
        } else {
            (name, args)
        };
        let mut guard = self.store.lock().await;
        if guard.is_none() {
            let roots = foreground_skill_creation_root(&self.config).and_then(|create_root| {
                super::review_pending::pending_root()
                    .map(|pending_root| (create_root, pending_root))
            });
            let (create_root, pending_root) = match roots {
                Ok(roots) => roots,
                Err(error) => return ToolExecutionResult::failure(error),
            };
            match super::review_skills::ReviewSkillStore::new(
                &self.workspace,
                &self.skill_paths,
                create_root,
                pending_root,
                self.config.write_approval,
                &self.run_id,
                &self.session_id,
            )
            .await
            {
                Ok(store) => *guard = Some(store),
                Err(error) => return ToolExecutionResult::failure(error),
            }
        }
        match guard
            .as_mut()
            .expect("initialized skill store")
            .execute(name, args)
            .await
        {
            Ok(value) => ToolExecutionResult::success(value.to_string(), None),
            Err(error) => ToolExecutionResult::failure(error),
        }
    }
}

// Match review's home-relative creation policy, never the active workspace.
fn foreground_skill_creation_root(config: &ReviewConfig) -> Result<std::path::PathBuf, String> {
    super::skills::review_creation_root(config)
}

fn is_review_skill_tool(name: &str) -> bool {
    matches!(name, "skills_list" | "skill_view" | "skill_manage")
}

/// Outcome of dispatching one tool call issued inside a subagent.
struct SubagentToolOutcome {
    result: ToolExecutionResult,
    /// Skill activated by this call, so the caller can emit the UI event.
    activated_skill: Option<SkillInfo>,
}

fn selected_skill_names(messages: &[ChatMessage]) -> Result<Vec<String>, String> {
    let Some(latest) = messages.iter().rfind(|message| message.role == "user") else {
        return Ok(Vec::new());
    };
    if latest.selected_skills.len() > 5 {
        return Err("Select at most five skills for the next turn.".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut names = Vec::new();
    for name in &latest.selected_skills {
        let key = name.to_ascii_lowercase();
        super::reflection::validate_name(&key)
            .map_err(|error| format!("Invalid selected skill name: {error}"))?;
        if seen.insert(key.clone()) {
            names.push(key);
        }
    }
    Ok(names)
}

async fn load_selected_skill_context(
    messages: &[ChatMessage],
    catalog: &[SkillInfo],
    emitter: &EmitterContext,
) -> Result<String, String> {
    let names = selected_skill_names(messages)?;
    let mut context = String::new();
    let mut activated = Vec::new();
    for name in names {
        let skill = find_skill(catalog, &name)
            .ok_or_else(|| format!("Selected skill '{name}' was not found in the catalog."))?;
        let instructions = read_skill_content(skill)
            .await
            .map_err(|error| format!("Could not load selected skill '{}': {error}", skill.name))?;
        let header = format!("\n\n## Selected skill: {}\n", skill.name);
        context
            .len()
            .checked_add(header.len())
            .and_then(|length| length.checked_add(instructions.len()))
            .filter(|length| *length <= 256 * 1024)
            .ok_or("Selected skill instructions exceed the aggregate 256 KiB limit.")?;
        context.push_str(&header);
        context.push_str(&instructions);
        activated.push(skill.clone());
    }
    for skill in activated {
        emitter.skill(skill);
    }
    Ok(context)
}

fn messages_with_skill_context(messages: &[ChatMessage], context: &str) -> Vec<ChatMessage> {
    let mut messages = messages.to_vec();
    if let Some(latest) = messages.iter_mut().rfind(|message| message.role == "user") {
        latest.content.push_str(context);
    }
    messages
}

/// Outcome of resolving a `use_skill` tool call against the skill catalog.
struct SkillActivation {
    result: ToolExecutionResult,
    /// The skill that was found, so callers can emit the activation event.
    skill: Option<SkillInfo>,
}

/// Shared implementation of the `use_skill` tool.
///
/// The parent run and subagents must behave identically, so both dispatch
/// through here. The SKILL.md body is always read through the validated
/// catalog path, never through a model-supplied filesystem path.
async fn activate_skill(
    skills: &mut Vec<SkillInfo>,
    name: &str,
    skill_paths: &[String],
) -> SkillActivation {
    let skill = if let Some(s) = find_skill(skills, name) {
        Some(s.clone())
    } else {
        // Dynamic hot-reload: re-scan the catalog in case a skill was learned or created mid-session
        *skills = load_skills(skill_paths).await;
        find_skill(skills, name).cloned()
    };

    let Some(skill) = skill else {
        return SkillActivation {
            result: ToolExecutionResult::failure(format!(
                "Skill '{name}' was not found in the catalog."
            )),
            skill: None,
        };
    };
    let result = match read_skill_content(&skill).await {
        Ok(instructions) => ToolExecutionResult::success(
            format!(
                "Skill '{}' activated from `{}`.\n\nInstructions:\n{}",
                skill.name, skill.path, instructions
            ),
            None,
        ),
        Err(error) => ToolExecutionResult::failure(error),
    };
    SkillActivation {
        result,
        skill: Some(skill),
    }
}

/// Dispatches a single tool call made from inside a subagent.
///
/// Subagents reuse the native tool surface (`spawn_subagent` is filtered out of
/// their advertised tool list upstream), so every other tool has to behave
/// exactly as it does in the parent run.
async fn dispatch_subagent_tool(
    name: &str,
    args: &serde_json::Value,
    context: &ToolContext,
    skills: &mut Vec<SkillInfo>,
    skill_paths: &[String],
    execution_mode: ExecutionMode,
) -> SubagentToolOutcome {
    if let Some(error) = tool_policy_error(execution_mode, name, args, false) {
        return SubagentToolOutcome {
            result: ToolExecutionResult::failure(error),
            activated_skill: None,
        };
    }
    // Review maintenance needs a persistent, configured parent-run store. Do not
    // silently fall through to unknown tools or create a fresh store per call.
    if is_review_skill_tool(name) || name == "learn_skill" {
        return SubagentToolOutcome {
            result: ToolExecutionResult::failure("Skill maintenance tools are only available in the parent run; report the proposed change to the parent.".into()),
            activated_skill: None,
        };
    }
    if name == "use_skill" {
        let skill_name = args
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let activation = activate_skill(skills, skill_name, skill_paths).await;
        SubagentToolOutcome {
            result: activation.result,
            activated_skill: activation.skill,
        }
    } else {
        let result = execute_tool(name, args, context).await;
        SubagentToolOutcome {
            result,
            activated_skill: None,
        }
    }
}

fn safe_prefix(s: &str, char_count: usize) -> &str {
    match s.char_indices().nth(char_count) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

fn is_substantive_finding(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if trimmed.len() < 80
        && (lower.starts_with("let me ")
            || lower.starts_with("i will ")
            || lower.starts_with("running ")
            || lower.starts_with("checking ")
            || lower.starts_with("executing "))
    {
        return false;
    }
    true
}

struct SubagentParams<'a> {
    role: &'a str,
    task: &'a str,
    subpath: Option<&'a str>,
    skill_paths: &'a [String],
    inherited_policy: &'a str,
    execution_mode: ExecutionMode,
    completion_options: &'a CompletionOptions,
}

fn scope_subagent_artifact(scope: &str, mut artifact: Artifact) -> Artifact {
    artifact.id = format!("{scope}/{}", artifact.id);
    artifact
}

fn scope_subagent_tool_event(scope: &str, event: ToolEvent) -> ToolEvent {
    match event {
        ToolEvent::Artifact(artifact) => {
            ToolEvent::Artifact(scope_subagent_artifact(scope, artifact))
        }
        ToolEvent::TerminalOutput {
            artifact_id,
            delta,
            stream,
        } => ToolEvent::TerminalOutput {
            artifact_id: format!("{scope}/{artifact_id}"),
            delta,
            stream,
        },
    }
}

fn subagent_completion_options(parent: &CompletionOptions) -> CompletionOptions {
    parent.clone()
}

fn mode_instructions(mode: ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::RequestReview => "\n## Permission & Execution Mode: REQUEST REVIEW\nNative enforcement rejects write, execute, task mutation and MCP calls. No native approval flow exists; asking in prose does not grant permission.",
        ExecutionMode::Default => "\n## Permission & Execution Mode: DEFAULT\nAsk before sensitive or destructive operations. Standard non-destructive reads and edits can proceed directly.",
        ExecutionMode::Auto => "\n## Permission & Execution Mode: AUTO (PREVIEW)\nProceed autonomously on routine actions, but ask before operations with significant risk of data loss.",
        ExecutionMode::Turbo => "\n## Permission & Execution Mode: TURBO\nRun tools autonomously unless the operator imposes additional restrictions.",
    }
}

fn inherited_run_policy(
    operator: Option<&str>,
    memory: Option<&str>,
    mode: ExecutionMode,
) -> String {
    let mut policy = LEAN_WORKFLOW.to_string();
    if let Some(operator) = operator.filter(|value| !value.trim().is_empty()) {
        policy.push_str(&format!("\n## Operator instructions\n{operator}"));
    }
    if let Some(memory) = memory {
        policy.push_str(&format!("\n## Workspace AGENTS.md instructions\n{memory}"));
    }
    policy.push_str(mode_instructions(mode));
    policy
}

fn mcp_connections_allowed(mode: ExecutionMode) -> bool {
    mode != ExecutionMode::RequestReview
}

fn tool_policy_error(
    mode: ExecutionMode,
    name: &str,
    args: &serde_json::Value,
    is_mcp: bool,
) -> Option<String> {
    if mode != ExecutionMode::RequestReview {
        return None;
    }
    let read_only = !is_mcp
        && (matches!(
            name,
            "read_file"
                | "list_directory"
                | "search_files"
                | "fetch_web_page"
                | "use_skill"
                | "skills_list"
                | "skill_view"
                | "spawn_subagent"
        ) || (name == "manage_task"
            && matches!(
                args.get("action").and_then(|v| v.as_str()),
                Some("list" | "status")
            )));
    (!read_only).then(|| format!("Request Review blocked tool '{name}' before execution: native approval is not available."))
}

fn subagent_workspace(
    workspace: &str,
    subpath: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    if workspace.trim().is_empty() {
        return Err("Subagent requires an active workspace".into());
    }
    let target = super::tools::resolve_workspace_path(workspace, subpath.unwrap_or("."))?;
    if !target.is_dir() {
        return Err("Subagent subpath must be an existing directory inside the workspace".into());
    }
    Ok(target)
}

async fn subagent_system_prompt(
    role: &str,
    task: &str,
    workspace: &str,
    target: &std::path::Path,
    inherited: &str,
) -> Result<String, String> {
    let mut system = format!("You are an autonomous subagent specialized as '{role}'.\nWorking directory: {}\nTask: {task}\nFollow inherited operator and workspace constraints. Investigate with tools, then provide a standalone final report without tools.\n{inherited}", target.display());
    let root = super::tools::resolve_workspace_path(workspace, ".")?;
    let relative = target
        .strip_prefix(&root)
        .map_err(|_| "Subagent target escaped workspace")?;
    let mut prefix = std::path::PathBuf::new();
    for component in relative.components() {
        prefix.push(component);
        let relative_agents = prefix.join("AGENTS.md");
        let path =
            super::tools::resolve_workspace_path(workspace, &relative_agents.to_string_lossy())?;
        if path.exists() {
            let parent = path
                .parent()
                .ok_or("AGENTS.md has no parent")?
                .to_string_lossy();
            if let Some(memory) = load_workspace_memory(&parent).await? {
                system.push_str(&format!(
                    "\n## Additional workspace instructions ({})\n{memory}",
                    relative_agents.display()
                ));
            }
        }
    }
    Ok(system)
}

fn subagent_synthesis_failure(
    result: Result<super::provider::StreamResult, String>,
    text: &str,
    cancel: &CancellationToken,
    completed_cleanly: bool,
) -> Option<ToolExecutionResult> {
    let cancelled = cancel.is_cancelled() || result.as_ref().is_ok_and(|result| result.cancelled);
    let error = if cancelled {
        Some("Subagent cancelled during final synthesis".to_string())
    } else {
        match result {
            Err(error) => Some(format!("Subagent final synthesis failed: {error}")),
            Ok(_) if !completed_cleanly => {
                Some("Subagent incomplete: investigation turn budget exhausted".into())
            }
            Ok(_) if text.trim().is_empty() => {
                Some("Subagent final synthesis returned an empty report".into())
            }
            Ok(_) => None,
        }
    };
    error.map(|error| ToolExecutionResult {
        output: format!("{error}\n\nPartial findings (not a completed report):\n{text}"),
        cancelled,
        success: false,
        ..Default::default()
    })
}

async fn run_subagent(
    emitter: &EmitterContext,
    provider: &ProviderConfig,
    workspace_cwd: &str,
    params: SubagentParams<'_>,
    parent_cancel: CancellationToken,
) -> ToolExecutionResult {
    let subagent_id = format!("sub-{}", &Uuid::new_v4().to_string()[..8]);
    let role = params.role;
    let task = params.task;
    let subpath = params.subpath;
    let configured_skill_paths = params.skill_paths;
    let target_dir = match subagent_workspace(workspace_cwd, subpath) {
        Ok(path) => path,
        Err(error) => return ToolExecutionResult::failure(error),
    };
    let cwd_str = target_dir.to_string_lossy().to_string();

    let mut progress = SubagentProgress {
        id: subagent_id.clone(),
        role: role.to_string(),
        task: task.to_string(),
        status: "running".into(),
        started_at: chrono::Utc::now().timestamp_millis(),
        completed_at: None,
        summary: None,
        artifact_id: None,
    };
    emitter.subagent(progress.clone());

    let mut subagent_system = match subagent_system_prompt(
        role,
        task,
        workspace_cwd,
        &target_dir,
        params.inherited_policy,
    )
    .await
    {
        Ok(system) => system,
        Err(error) => {
            progress.status = "failed".into();
            progress.summary = Some(error.clone());
            progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
            emitter.subagent(progress);
            return ToolExecutionResult::failure(error);
        }
    };
    let all_skill_paths =
        super::skills::workspace_skill_paths(workspace_cwd, configured_skill_paths);
    let mut skills = load_skills(&all_skill_paths).await;
    subagent_system.push_str(&render_skill_catalog(&skills));

    let mut conversation = Vec::new();
    conversation.push(serde_json::json!({"role": "system", "content": subagent_system}));
    conversation.push(serde_json::json!({"role": "user", "content": format!("Execute the task and report your findings in full:\n\n{task}")}));

    let tools_value = get_agent_tools();
    let sub_tools: Vec<serde_json::Value> = tools_value
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| {
            t.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .map(|n| n != "spawn_subagent" && n != "learn_skill" && !is_review_skill_tool(n))
                .unwrap_or(true)
        })
        .collect();
    let tools = serde_json::Value::Array(sub_tools);

    let options = subagent_completion_options(params.completion_options);

    let child_emitter = emitter.clone();
    let child_scope = subagent_id.clone();
    let sink =
        Arc::new(
            move |event: ToolEvent| match scope_subagent_tool_event(&child_scope, event) {
                ToolEvent::Artifact(artifact) => child_emitter.artifact(artifact),
                ToolEvent::TerminalOutput {
                    artifact_id,
                    delta,
                    stream,
                } => child_emitter.terminal(artifact_id, delta, stream),
            },
        );
    let sub_context = ToolContext {
        session_id: emitter.session_id.clone(),
        cwd: cwd_str,
        cancel: parent_cancel.clone(),
        sink: Some(sink),
    };

    let mut turns = 0;
    let mut final_text = String::new();
    let mut intermediate_findings: Vec<String> = Vec::new();
    let mut completed_cleanly = false;
    let max_turns = SUBAGENT_MAX_TURNS;

    while turns < max_turns {
        if parent_cancel.is_cancelled() {
            progress.status = "cancelled".into();
            progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
            emitter.subagent(progress);
            return ToolExecutionResult {
                output: "Subagent cancelled by operator".into(),
                artifact: None,
                success: false,
                cancelled: true,
                ..Default::default()
            };
        }
        turns += 1;
        let mut turn_text = String::new();
        let mut partial_tools: HashMap<usize, ToolCallItem> = HashMap::new();
        let mut subagent_attempt = 1;
        let mut final_subagent_result = None;
        let mut subagent_last_err = String::new();

        while subagent_attempt <= 3 {
            if parent_cancel.is_cancelled() {
                break;
            }
            if subagent_attempt > 1 {
                let backoff =
                    std::time::Duration::from_millis(1000 * (1 << (subagent_attempt - 2)));
                tokio::select! {
                    _ = parent_cancel.cancelled() => break,
                    _ = tokio::time::sleep(backoff) => {}
                }
                if parent_cancel.is_cancelled() {
                    break;
                }
            }

            turn_text.clear();
            partial_tools.clear();

            let attempt_result = stream_chat_completion(
                provider,
                &serde_json::Value::Array(conversation.clone()),
                &tools,
                &options,
                |chunk| {
                    turn_text.push_str(&chunk);
                },
                |_reasoning| {},
                |delta: ToolDelta| {
                    let item = partial_tools
                        .entry(delta.index)
                        .or_insert_with(|| ToolCallItem {
                            id: format!("sub_tool_{}", delta.index),
                            name: "tool".into(),
                            args: serde_json::Value::Object(Default::default()),
                            status: "pending".into(),
                            result: None,
                            artifact_id: None,
                            arguments_raw: Some(String::new()),
                            ..Default::default()
                        });
                    if let Some(name) = delta.name.filter(|v| !v.is_empty()) {
                        item.name = name;
                    }
                    if let Some(args) = delta.arguments {
                        item.arguments_raw = Some(format!(
                            "{}{}",
                            item.arguments_raw.clone().unwrap_or_default(),
                            args
                        ));
                    }
                },
                parent_cancel.clone(),
            )
            .await;

            match attempt_result {
                Ok(r) => {
                    final_subagent_result = Some(r);
                    break;
                }
                Err(err) => {
                    subagent_last_err = err.clone();
                    if is_retryable_provider_error(&err)
                        && subagent_attempt < 3
                        && !parent_cancel.is_cancelled()
                    {
                        subagent_attempt += 1;
                        continue;
                    } else {
                        break;
                    }
                }
            }
        }

        let result = match final_subagent_result {
            Some(r) => r,
            None => {
                if parent_cancel.is_cancelled() {
                    progress.status = "cancelled".into();
                    progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
                    emitter.subagent(progress);
                    return ToolExecutionResult {
                        output: "Subagent cancelled by operator".into(),
                        artifact: None,
                        success: false,
                        cancelled: true,
                        ..Default::default()
                    };
                }
                let err = if subagent_last_err.is_empty() {
                    "Subagent stream cancelled".to_string()
                } else if subagent_attempt > 1 {
                    format!(
                        "Subagent failed after {subagent_attempt} attempts: {subagent_last_err}"
                    )
                } else {
                    subagent_last_err
                };
                progress.status = "failed".into();
                progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
                progress.summary = Some(format!("Error: {err}"));
                emitter.subagent(progress);
                return ToolExecutionResult::failure(format!(
                    "Subagent encountered provider error: {err}"
                ));
            }
        };

        if result.cancelled || parent_cancel.is_cancelled() {
            progress.status = "cancelled".into();
            progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
            emitter.subagent(progress);
            return ToolExecutionResult {
                output: "Subagent cancelled by operator".into(),
                cancelled: true,
                ..Default::default()
            };
        }
        conversation.push(result.assistant_message(provider.kind));
        if result.tool_calls.is_empty() {
            final_text = result.content.clone();
            completed_cleanly = true;
            break;
        }

        if is_substantive_finding(&turn_text) {
            intermediate_findings.push(turn_text.trim().to_string());
        }

        for call in &result.tool_calls {
            let started_at = chrono::Utc::now().timestamp_millis();
            let args = if call.function.arguments.trim().is_empty() {
                Ok(serde_json::json!({}))
            } else {
                serde_json::from_str::<serde_json::Value>(&call.function.arguments)
            };
            let item_args = args
                .as_ref()
                .cloned()
                .unwrap_or_else(|_| serde_json::json!({"raw":call.function.arguments}));
            let mut tool_item = ToolCallItem {
                id: format!("{subagent_id}/turn_{turns}/{}", call.stream_index),
                name: call.function.name.clone(),
                args: item_args,
                status: "running".into(),
                arguments_raw: Some(call.function.arguments.clone()),
                started_at: Some(started_at),
                ..Default::default()
            };
            emitter.tool(tool_item.clone());
            let outcome = match args {
                Ok(args) if args.is_object() => {
                    dispatch_subagent_tool(
                        &call.function.name,
                        &args,
                        &sub_context,
                        &mut skills,
                        &all_skill_paths,
                        params.execution_mode,
                    )
                    .await
                }
                _ => SubagentToolOutcome {
                    result: ToolExecutionResult::failure(
                        "Subagent tool arguments must be a valid JSON object".into(),
                    ),
                    activated_skill: None,
                },
            };
            if let Some(skill) = outcome.activated_skill {
                emitter.skill(skill);
            }
            let mut execution = outcome.result;
            if let Some(artifact) = execution.artifact.take() {
                let artifact = scope_subagent_artifact(&subagent_id, artifact);
                tool_item.artifact_id = Some(artifact.id.clone());
                emitter.artifact(artifact);
            }
            let completed_at = chrono::Utc::now().timestamp_millis();
            tool_item.status = if execution.cancelled || parent_cancel.is_cancelled() {
                "cancelled"
            } else if execution.success {
                "done"
            } else {
                "error"
            }
            .into();
            tool_item.result = Some(execution.output.clone());
            tool_item.completed_at = Some(completed_at);
            tool_item.duration_ms = execution
                .duration_ms
                .or(Some((completed_at - started_at).max(0) as u64));
            tool_item.exit_code = execution.exit_code;
            tool_item.terminal_output = execution.terminal_output.clone();
            tool_item.files_changed = execution.files_changed.clone();
            emitter.tool(tool_item);
            conversation.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": execution.output
            }));
        }
    }

    if parent_cancel.is_cancelled() {
        progress.status = "cancelled".into();
        progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
        emitter.subagent(progress);
        return ToolExecutionResult {
            output: "Subagent cancelled by operator".into(),
            artifact: None,
            success: false,
            cancelled: true,
            ..Default::default()
        };
    }

    let needs_synthesis = !completed_cleanly
        || final_text.trim().is_empty()
        || (final_text.trim().len() < 120 && (!intermediate_findings.is_empty() || turns > 1));

    if needs_synthesis {
        let synthesis_prompt = if !intermediate_findings.is_empty() {
            format!(
                "All tool executions and investigation steps are complete. Below are intermediate findings recorded during the run:\n\n{}\n\nProvide your final, comprehensive, and complete report now. Include all steps, findings, and conclusions in full without truncation.",
                intermediate_findings.join("\n\n")
            )
        } else {
            "All tool executions and investigation steps are complete. Provide your final comprehensive report of findings and conclusions now. Include all steps in full without truncation.".to_string()
        };

        conversation.push(serde_json::json!({
            "role": "user",
            "content": synthesis_prompt
        }));

        let mut synthesis_text = String::new();
        let synth_options = subagent_completion_options(&options);

        let synth_res = stream_chat_completion(
            provider,
            &serde_json::Value::Array(conversation.clone()),
            &serde_json::json!([]),
            &synth_options,
            |chunk| {
                synthesis_text.push_str(&chunk);
            },
            |_reasoning| {},
            |_delta| {},
            parent_cancel.clone(),
        )
        .await;

        let partial = [
            intermediate_findings.join("\n\n"),
            final_text.clone(),
            synthesis_text.clone(),
        ]
        .join("\n\n");
        if let Some(mut failure) = subagent_synthesis_failure(
            synth_res,
            &synthesis_text,
            &parent_cancel,
            completed_cleanly,
        ) {
            failure.output.push_str(&format!("\n\n{partial}"));
            failure.output = redact_provider_error(failure.output, provider);
            progress.status = if failure.cancelled {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
            progress.summary = Some(failure.output.clone());
            emitter.subagent(progress);
            return failure;
        }
        final_text = synthesis_text;
    }

    let report = if final_text.trim().is_empty() {
        if !intermediate_findings.is_empty() {
            intermediate_findings.join("\n\n")
        } else {
            format!("Subagent completed exploration in {turns} turns.")
        }
    } else if !intermediate_findings.is_empty() {
        let mut missing: Vec<&str> = Vec::new();
        for finding in &intermediate_findings {
            let sample = safe_prefix(finding, 40);
            if !final_text.contains(sample) {
                missing.push(finding.as_str());
            }
        }
        if !missing.is_empty() && final_text.len() < 1200 {
            format!("{}\n\n{}", missing.join("\n\n"), final_text)
        } else {
            final_text
        }
    } else {
        final_text
    };

    let artifact_id = format!("art-{}", Uuid::new_v4());
    let artifact = Artifact {
        stream_revision: None,
        id: artifact_id.clone(),
        title: format!("Subagent: {}", role),
        artifact_type: "markdown".into(),
        language: Some("markdown".into()),
        content: format!("# Subagent Report: {role}\n**Task**: {task}\n**Status**: Completed\n\n## Findings\n{report}"),
        file_path: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    };
    emitter.artifact(artifact.clone());

    progress.status = "completed".into();
    progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
    progress.summary = Some(report.clone());
    progress.artifact_id = Some(artifact_id);
    emitter.subagent(progress);

    ToolExecutionResult::success(
        format!("Subagent [{role}] completed successfully.\n\nSummary of findings:\n{report}"),
        Some(artifact),
    )
}

const MAX_WORKSPACE_MEMORY_BYTES: u64 = 1024 * 1024;

async fn load_workspace_memory(workspace_cwd: &str) -> Result<Option<String>, String> {
    if workspace_cwd.trim().is_empty() {
        return Ok(None);
    }
    let path = super::tools::resolve_workspace_path(workspace_cwd, "AGENTS.md")?;
    if !path.exists() {
        return Ok(None);
    }
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|error| format!("Could not inspect AGENTS.md: {error}"))?;
    if !metadata.is_file() {
        return Err("AGENTS.md is not a regular file".into());
    }
    if metadata.len() > MAX_WORKSPACE_MEMORY_BYTES {
        return Err("AGENTS.md exceeds the 1 MiB context limit".into());
    }
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|error| format!("Could not read AGENTS.md: {error}"))?;
    Ok((!content.trim().is_empty()).then_some(content))
}

struct ReviewState {
    id: String,
    run_id: String,
    cancel: CancellationToken,
}

#[derive(Default)]
struct ReviewCadence {
    iterations: HashMap<String, usize>,
}

impl ReviewCadence {
    fn iteration(&mut self, session_id: &str) {
        let count = self.iterations.entry(session_id.to_string()).or_default();
        *count = count.saturating_add(1);
    }

    fn tool_attempt(&mut self, session_id: &str, tool: &str) {
        if matches!(tool, "learn_skill" | "skill_manage") {
            self.iterations.insert(session_id.to_string(), 0);
        }
    }

    fn take_due(&mut self, session_id: &str, interval: usize) -> bool {
        let count = self.iterations.entry(session_id.to_string()).or_default();
        if interval > 0 && *count >= interval {
            *count = 0;
            true
        } else {
            false
        }
    }
}

#[derive(Default)]
struct UpdateGate {
    updating: bool,
    in_flight: usize,
}

/// Counts admitted native work independently of cancellable registry entries.
struct NativeWorkLease {
    gate: Arc<Mutex<UpdateGate>>,
}

impl Drop for NativeWorkLease {
    fn drop(&mut self) {
        self.gate
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .in_flight -= 1;
    }
}

/// Exclusive native-update reservation. Keep this alive through the running-task
/// check and installation; dropping it reopens run and review admission.
#[must_use = "dropping the permit reopens agent admission"]
pub struct UpdatePermit {
    gate: Arc<Mutex<UpdateGate>>,
}

impl Drop for UpdatePermit {
    fn drop(&mut self) {
        self.gate
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .updating = false;
    }
}

pub struct AgentRunner {
    // Admission always locks gate -> active -> reviews. Keep the gate locked
    // through registration so readiness and new work cannot race.
    update_gate: Arc<Mutex<UpdateGate>>,
    active: Arc<Mutex<HashMap<String, (String, CancellationToken)>>>,
    reviews: Arc<Mutex<HashMap<String, ReviewState>>>,
    review_cadence: Arc<Mutex<ReviewCadence>>,
}

impl Default for AgentRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRunner {
    pub fn new() -> Self {
        Self {
            update_gate: Arc::new(Mutex::new(UpdateGate::default())),
            active: Arc::new(Mutex::new(HashMap::new())),
            reviews: Arc::new(Mutex::new(HashMap::new())),
            review_cadence: Arc::new(Mutex::new(ReviewCadence::default())),
        }
    }

    /// Reserve quiescent native work before checking global background tasks.
    /// Cancelled reviews still block after removal until their leases finish.
    pub fn acquire_update_permit(&self) -> Result<UpdatePermit, String> {
        let mut gate = self
            .update_gate
            .lock()
            .map_err(|_| "Update gate is unavailable")?;
        if gate.updating {
            return Err("An update is already in progress".into());
        }
        let active = self
            .active
            .lock()
            .map_err(|_| "Agent run registry is unavailable")?;
        let reviews = self
            .reviews
            .lock()
            .map_err(|_| "Review registry is unavailable")?;
        if gate.in_flight != 0 || !active.is_empty() || !reviews.is_empty() {
            return Err("Wait for active runs and self-improvement reviews to finish before updating".into());
        }
        gate.updating = true;
        Ok(UpdatePermit {
            gate: self.update_gate.clone(),
        })
    }

    pub fn begin(&self, run_id: &str, session_id: &str) -> Result<CancellationToken, String> {
        if run_id.trim().is_empty() || session_id.trim().is_empty() {
            return Err("runId and sessionId are required".into());
        }
        let gate = self
            .update_gate
            .lock()
            .map_err(|_| "Update gate is unavailable")?;
        if gate.updating {
            return Err("An update is in progress; wait before starting a run".into());
        }
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Agent run registry is unavailable".to_string())?;
        if active.contains_key(run_id) {
            return Err(format!("Run {run_id} is already active"));
        }
        if active
            .values()
            .any(|(active_session, _)| active_session == session_id)
        {
            return Err("This session already has an active run".into());
        }
        let mut reviews = self
            .reviews
            .lock()
            .map_err(|_| "Review registry is unavailable")?;
        if let Some(review) = reviews.remove(session_id) {
            review.cancel.cancel();
        }
        let token = CancellationToken::new();
        active.insert(run_id.to_string(), (session_id.to_string(), token.clone()));
        Ok(token)
    }

    fn begin_review(
        &self,
        run_id: &str,
        session_id: &str,
        foreground: Option<&CancellationToken>,
    ) -> Result<ReflectionLease, String> {
        // Both manual and automatic reviews share update-gated admission.
        let mut gate = self
            .update_gate
            .lock()
            .map_err(|_| "Update gate is unavailable")?;
        if gate.updating {
            return Err("An update is in progress; wait before starting a review".into());
        }
        let active = self
            .active
            .lock()
            .map_err(|_| "Run registry is unavailable")?;
        if active.iter().any(|(id, (session, _))| {
            session == session_id && (foreground.is_none() || id != run_id)
        }) {
            return Err("Stop the active run before reviewing this session".into());
        }
        if foreground.is_some_and(CancellationToken::is_cancelled) {
            return Err("Review was cancelled".into());
        }
        let mut reviews = self
            .reviews
            .lock()
            .map_err(|_| "Review registry is unavailable")?;
        if reviews.contains_key(session_id) {
            return Err("A self-improvement review is already running for this session".into());
        }
        if reviews.len() >= 4 {
            return Err("Wait for an active self-improvement review to finish".into());
        }
        let id = Uuid::new_v4().to_string();
        let cancel = foreground
            .map(CancellationToken::child_token)
            .unwrap_or_default();
        reviews.insert(
            session_id.to_string(),
            ReviewState {
                id: id.clone(),
                run_id: run_id.to_string(),
                cancel: cancel.clone(),
            },
        );
        gate.in_flight += 1;
        Ok(ReflectionLease {
            _work: NativeWorkLease {
                gate: self.update_gate.clone(),
            },
            registry: self.reviews.clone(),
            session_id: session_id.to_string(),
            id,
            cancel,
        })
    }

    /// Transfer foreground work to a detached worker before done/finish can
    /// retire its active entry. The caller still owns foreground admission.
    fn reserve_finalization(&self) -> NativeWorkLease {
        let mut gate = self
            .update_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert!(
            !gate.updating,
            "foreground finalization must be reserved before the run finishes"
        );
        gate.in_flight += 1;
        NativeWorkLease {
            gate: self.update_gate.clone(),
        }
    }

    pub fn abort(&self, run_id: &str) -> Result<(), String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "Agent run registry is unavailable".to_string())?;
        if let Some((_, token)) = active.get(run_id) {
            token.cancel();
        }
        let reviews = self
            .reviews
            .lock()
            .map_err(|_| "Review registry is unavailable")?;
        for review in reviews.values().filter(|review| review.run_id == run_id) {
            review.cancel.cancel();
        }
        Ok(())
    }

    pub fn finish(&self, run_id: &str) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(run_id);
        }
    }

    pub async fn review_session(
        &self,
        app: AgentHost,
        session_id: String,
        provider: ProviderConfig,
    ) -> Result<(), String> {
        // Reserve before the first await: a foreground run must be able to cancel
        // this review even while its saved session/settings are being loaded.
        let lease = self.begin_review(&format!("manual-{}", Uuid::new_v4()), &session_id, None)?;
        let data = super::storage::load_session(&session_id).await?;
        let cwd = data.session.workspace_cwd.unwrap_or_default();
        let (run_id, snapshot) = completed_review_snapshot(&data.messages)?;
        lease.set_run_id(&run_id)?;
        let settings = super::storage::load_settings().await?;
        let options = CompletionOptions {
            thinking: settings.thinking,
            reasoning_effort: settings.reasoning_effort,
            temperature: settings.temperature,
            max_tokens: settings.max_tokens,
            stream: settings.stream_response,
        };
        run_reflection(
            app,
            lease,
            ReflectionContext {
                cwd,
                provider,
                snapshot,
                run_id,
                session_id,
                config: settings.review_config,
                skill_paths: settings.skill_paths,
                parent_tools: Some(get_agent_tools()),
                parent_options: Some(options),
            },
        )
        .await
    }

    pub async fn run(
        &self,
        app: AgentHost,
        workspace_cwd: String,
        request: AgentRunRequest,
        cancel: CancellationToken,
    ) {
        let emitter = EmitterContext {
            app: app.clone(),
            run_id: request.run_id.clone(),
            session_id: request.session_id.clone(),
        };
        if let Err(error) =
            super::images::validate_request_images(&request.messages, &request.provider)
        {
            emitter.error(error);
            emitter.done(false, false);
            return;
        }
        let all_skill_paths = match effective_skill_paths(
            &workspace_cwd,
            &request.skill_paths,
            &request.review_config,
        ) {
            Ok(paths) => paths,
            Err(error) => {
                emitter.error(error);
                emitter.done(false, false);
                return;
            }
        };
        let skills = load_skills(&all_skill_paths).await;
        let selected_context =
            match load_selected_skill_context(&request.messages, &skills, &emitter).await {
                Ok(context) => context,
                Err(error) => {
                    emitter.error(error);
                    emitter.done(false, false);
                    return;
                }
            };
        let mcp_servers = if mcp_connections_allowed(request.execution_mode) {
            request.mcp_servers.as_slice()
        } else {
            &[]
        };
        let mcp = Arc::new(tokio::select! {
            _ = cancel.cancelled() => {
                emitter.done(true, false);
                return;
            }
            manager = McpManager::connect(mcp_servers, &workspace_cwd) => manager,
        });
        for warning in &mcp.warnings {
            emitter.artifact(Artifact {
                stream_revision: None,
                id: format!("art-{}", Uuid::new_v4()),
                title: "MCP connection warning".into(),
                artifact_type: "markdown".into(),
                language: Some("markdown".into()),
                content: warning.clone(),
                file_path: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
        }

        let default_prompt = "You are AetherAgent, a native expert developer assistant. Follow these strict guidelines:\n- Maintain a clean, minimalist developer aesthetic (like Cursor and Linear).\n- CRITICAL: Never use emojis, emotes, smileys, decorative symbols, or gamified AI tropes in responses.\n- Keep explanations concise, clear, and direct without conversational filler or chatbot pleasantries.\n- Inspect before editing, use native tools deliberately, and route long outputs to artifacts.\n- For long-running commands, dev servers, or background watchers, use run_command with isDaemon: true to run in the background. Use manage_task to inspect logs, send input, or terminate tasks.\n- When referencing or creating generated files (documents, scripts, reports, images, etc.), always provide a clickable Markdown link with the exact absolute file path (e.g. `[filename.ext](file:///D:/path/to/filename.ext)` or `[filename.ext](D:/path/to/filename.ext)`), so the user can click it to open the file directly in their operating system.";
        let mut system = match request
            .system_prompt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(custom) => format!("{default_prompt}\n\n## Operator instructions\n{custom}"),
            None => default_prompt.to_string(),
        };
        system.push_str(LEAN_WORKFLOW);
        system.push_str(&render_skill_catalog(&skills));
        let skills_shared = Arc::new(RwLock::new(skills));
        let foreground_skill_tools = ForegroundSkillTools::new(
            &workspace_cwd,
            &all_skill_paths,
            &request.review_config,
            &request.run_id,
            &request.session_id,
        );
        let workspace_memory = match load_workspace_memory(&workspace_cwd).await {
            Ok(memory) => memory,
            Err(error) => {
                emitter.error(error);
                None
            }
        };
        if let Some(memory) = &workspace_memory {
            system.push_str(&format!("\n## Workspace AGENTS.md instructions\n{memory}"));
        }
        let inherited_policy = inherited_run_policy(
            request.system_prompt.as_deref(),
            workspace_memory.as_deref(),
            request.execution_mode,
        );
        system.push_str(&format!("\n## Active workspace\n{workspace_cwd}"));
        system.push_str(mode_instructions(request.execution_mode));

        let mut request_messages = messages_with_skill_context(&request.messages, &selected_context);
        if request.auto_compaction_enabled {
            let threshold = request.auto_compaction_threshold_tokens.unwrap_or(65_536);
            if let Some(compaction_evt) = apply_micro_compaction(
                &mut request_messages,
                &request.run_id,
                &request.session_id,
                threshold,
            ) {
                let _ = app.emit("agent:micro-compaction", &compaction_evt);
            }
        }

        let mut conversation = Vec::new();
        conversation.push(serde_json::json!({"role":"system","content":system}));
        for message in &request_messages {
            append_history_message(&mut conversation, message);
        }

        let tools_value = get_agent_tools();
        let mut tools = tools_value.as_array().cloned().unwrap_or_default();
        tools.extend(mcp.tools.clone());
        let tools = serde_json::Value::Array(tools);
        let options = CompletionOptions {
            thinking: request.thinking,
            reasoning_effort: request.reasoning_effort,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: request.stream_response,
        };
        let current_run_start = conversation.len();
        let mut turns = 0;
        let mut finished = false;
        let mut final_response_nonempty = false;
        while turns < request.max_turns.clamp(1, 100) {
            if cancel.is_cancelled() {
                break;
            }
            if request.review_config.creation_nudge_interval > 0 {
                if let Ok(mut cadence) = self.review_cadence.lock() {
                    cadence.iteration(&request.session_id);
                }
            }
            turns += 1;
            let max_provider_attempts = 3;
            let mut current_attempt = 1;
            let mut last_provider_error = String::new();
            let mut final_stream_result: Option<super::provider::StreamResult> = None;

            while current_attempt <= max_provider_attempts {
                if cancel.is_cancelled() {
                    break;
                }

                if current_attempt > 1 {
                    let backoff =
                        std::time::Duration::from_millis(1000 * (1 << (current_attempt - 2)));
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = tokio::time::sleep(backoff) => {}
                    }
                    if cancel.is_cancelled() {
                        break;
                    }
                }

                let mut partial_tools: HashMap<usize, ToolCallItem> = HashMap::new();
                let chunk_emitter = emitter.clone();
                let reasoning_emitter = emitter.clone();
                let tool_emitter = emitter.clone();
                let attempt_had_chunks =
                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let had_chunks_clone1 = attempt_had_chunks.clone();
                let had_chunks_clone2 = attempt_had_chunks.clone();
                let had_chunks_clone3 = attempt_had_chunks.clone();

                let result = stream_chat_completion(
                    &request.provider,
                    &serde_json::Value::Array(conversation.clone()),
                    &tools,
                    &options,
                    move |chunk| {
                        had_chunks_clone1.store(true, std::sync::atomic::Ordering::Relaxed);
                        chunk_emitter.chunk(chunk);
                    },
                    move |reasoning| {
                        had_chunks_clone2.store(true, std::sync::atomic::Ordering::Relaxed);
                        reasoning_emitter.reasoning(reasoning);
                    },
                    |delta: ToolDelta| {
                        had_chunks_clone3.store(true, std::sync::atomic::Ordering::Relaxed);
                        let item =
                            partial_tools
                                .entry(delta.index)
                                .or_insert_with(|| ToolCallItem {
                                    id: format!(
                                        "turn_{turns}_attempt_{current_attempt}_stream_{}",
                                        delta.index
                                    ),
                                    name: "tool".into(),
                                    args: serde_json::Value::Object(Default::default()),
                                    status: "pending".into(),
                                    result: None,
                                    artifact_id: None,
                                    arguments_raw: Some(String::new()),
                                    started_at: Some(chrono::Utc::now().timestamp_millis()),
                                    ..Default::default()
                                });
                        if let Some(name) = delta.name.filter(|value| !value.is_empty()) {
                            item.name = name;
                        }
                        if let Some(arguments) = delta.arguments {
                            item.arguments_raw = Some(format!(
                                "{}{}",
                                item.arguments_raw.clone().unwrap_or_default(),
                                arguments
                            ));
                        }
                        tool_emitter.tool(item.clone());
                    },
                    cancel.clone(),
                )
                .await;

                match result {
                    Ok(res) => {
                        if current_attempt > 1 {
                            emitter.clear_notice();
                        }
                        final_stream_result = Some(res);
                        break;
                    }
                    Err(err) => {
                        let had_deltas =
                            attempt_had_chunks.load(std::sync::atomic::Ordering::Relaxed);
                        last_provider_error = err.clone();
                        let is_retryable = is_retryable_provider_error(&err);

                        if is_retryable
                            && current_attempt < max_provider_attempts
                            && !cancel.is_cancelled()
                        {
                            // Tutup placeholder upaya lama tanpa mengubah data parsial yang sudah tersimpan.
                            let completed_at = chrono::Utc::now().timestamp_millis();
                            let mut abandoned_tools: Vec<_> = partial_tools.into_iter().collect();
                            abandoned_tools.sort_unstable_by_key(|(index, _)| *index);
                            for (_, mut tool) in abandoned_tools {
                                tool.status = "cancelled".into();
                                tool.result = Some("Provider attempt abandoned for retry".into());
                                tool.completed_at = Some(completed_at);
                                tool.duration_ms = tool
                                    .started_at
                                    .map(|started_at| (completed_at - started_at).max(0) as u64);
                                emitter.tool(tool);
                            }
                            current_attempt += 1;
                            let notice_msg = format!(
                                "No output from provider for 60s — reconnecting (attempt {}/{})...",
                                current_attempt, max_provider_attempts
                            );
                            emitter.notice(
                                notice_msg,
                                Some(current_attempt),
                                Some(max_provider_attempts),
                                had_deltas,
                            );
                            continue;
                        } else {
                            break;
                        }
                    }
                }
            }

            if cancel.is_cancelled() {
                break;
            }

            let result = match final_stream_result {
                Some(result) => result,
                None => {
                    let err_msg = if last_provider_error.is_empty() {
                        "Provider stream was cancelled".to_string()
                    } else if current_attempt > 1 {
                        format!("Provider failed after {current_attempt} attempts: {last_provider_error}")
                    } else {
                        last_provider_error
                    };
                    emitter.error(redact_provider_error(err_msg, &request.provider));
                    break;
                }
            };
            if result.cancelled || cancel.is_cancelled() {
                break;
            }
            conversation.push(result.assistant_message(request.provider.kind));
            if result.tool_calls.is_empty() {
                final_response_nonempty = !result.content.trim().is_empty();
                emitter.provider_state(&conversation[current_run_start..]);
                finished = true;
                break;
            }

            let mut prepared_calls = Vec::with_capacity(result.tool_calls.len());
            for call in result.tool_calls.into_iter() {
                let ui_id = format!(
                    "turn_{turns}_attempt_{current_attempt}_stream_{}",
                    call.stream_index
                );
                let parsed_args = if call.function.arguments.trim().is_empty() {
                    Ok(serde_json::json!({}))
                } else {
                    serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                        .map_err(|error| error.to_string())
                };

                match parsed_args {
                    Ok(args) => {
                        let tool_item = ToolCallItem {
                            id: ui_id.clone(),
                            name: call.function.name.clone(),
                            args: args.clone(),
                            status: "running".into(),
                            result: None,
                            artifact_id: None,
                            arguments_raw: Some(call.function.arguments.clone()),
                            started_at: Some(chrono::Utc::now().timestamp_millis()),
                            completed_at: None,
                            duration_ms: None,
                            exit_code: None,
                            terminal_output: None,
                            files_changed: None,
                        };
                        emitter.tool(tool_item);
                        prepared_calls.push(PreparedCall {
                            call_id: call.id,
                            function_name: call.function.name,
                            arguments_raw: call.function.arguments,
                            ui_id,
                            args,
                            malformed_error: None,
                        });
                    }
                    Err(error) => {
                        let message = format!(
                            "Tool '{}' returned malformed JSON arguments: {error}",
                            call.function.name
                        );
                        emitter.tool(ToolCallItem {
                            id: ui_id.clone(),
                            name: call.function.name.clone(),
                            args: serde_json::json!({ "raw": call.function.arguments.clone() }),
                            status: "error".into(),
                            result: Some(message.clone()),
                            artifact_id: None,
                            arguments_raw: Some(call.function.arguments.clone()),
                            started_at: Some(chrono::Utc::now().timestamp_millis()),
                            completed_at: Some(chrono::Utc::now().timestamp_millis()),
                            duration_ms: Some(0),
                            exit_code: None,
                            terminal_output: None,
                            files_changed: None,
                        });
                        prepared_calls.push(PreparedCall {
                            call_id: call.id,
                            function_name: call.function.name,
                            arguments_raw: call.function.arguments,
                            ui_id,
                            args: serde_json::json!({}),
                            malformed_error: Some(message),
                        });
                    }
                }
            }

            let executed_results = dispatch_tool_batch(prepared_calls, &cancel, |prep| {
                // Reset on the attempt, including malformed or policy-denied calls.
                if let Ok(mut cadence) = self.review_cadence.lock() {
                    cadence.tool_attempt(&request.session_id, &prep.function_name);
                }
                let emitter = emitter.clone();
                let cancel = cancel.clone();
                let request_provider = request.provider.clone();
                let workspace_cwd = workspace_cwd.clone();
                let all_skill_paths = all_skill_paths.clone();
                let mcp = Arc::clone(&mcp);
                let skills_shared = Arc::clone(&skills_shared);
                let foreground_skill_tools = foreground_skill_tools.clone();
                let inherited_policy = inherited_policy.clone();
                let completion_options = options.clone();
                let execution_mode = request.execution_mode;

                async move {
                    if let Some(err) = prep.malformed_error {
                        return (
                            prep.call_id,
                            ToolExecutionResult {
                                output: err,
                                success: false,
                                cancelled: false,
                                ..Default::default()
                            },
                        );
                    }

                    if cancel.is_cancelled() {
                        let tool_item = ToolCallItem {
                            id: prep.ui_id,
                            name: prep.function_name,
                            args: prep.args,
                            status: "cancelled".into(),
                            result: Some("Cancelled by operator".into()),
                            artifact_id: None,
                            arguments_raw: Some(prep.arguments_raw),
                            started_at: Some(chrono::Utc::now().timestamp_millis()),
                            completed_at: Some(chrono::Utc::now().timestamp_millis()),
                            duration_ms: Some(0),
                            exit_code: None,
                            terminal_output: None,
                            files_changed: None,
                        };
                        emitter.tool(tool_item);
                        return (
                            prep.call_id,
                            ToolExecutionResult {
                                output: "Cancelled by operator".into(),
                                success: false,
                                cancelled: true,
                                ..Default::default()
                            },
                        );
                    }

                    let started_at = chrono::Utc::now().timestamp_millis();
                    let sink_emitter = emitter.clone();
                    let sink = Arc::new(move |event: ToolEvent| match event {
                        ToolEvent::Artifact(artifact) => sink_emitter.artifact(artifact),
                        ToolEvent::TerminalOutput {
                            artifact_id,
                            delta,
                            stream,
                        } => sink_emitter.terminal(artifact_id, delta, stream),
                    });
                    let context = ToolContext {
                        session_id: emitter.session_id.clone(),
                        cwd: workspace_cwd.clone(),
                        cancel: cancel.clone(),
                        sink: Some(sink),
                    };

                    let mut execution = if let Some(error) = tool_policy_error(
                        execution_mode,
                        &prep.function_name,
                        &prep.args,
                        mcp.contains(&prep.function_name),
                    ) {
                        ToolExecutionResult::failure(error)
                    } else if mcp.contains(&prep.function_name) {
                        let outcome = tokio::select! {
                            _ = cancel.cancelled() => Err("MCP call cancelled".to_string()),
                            result = mcp.call(&prep.function_name, prep.args.clone()) => result,
                        };
                        if cancel.is_cancelled() {
                            ToolExecutionResult {
                                output: "MCP call cancelled".into(),
                                success: false,
                                cancelled: true,
                                ..Default::default()
                            }
                        } else {
                            match outcome {
                                Ok(value) => {
                                    let text = result_text(&value);
                                    let is_error = value
                                        .get("isError")
                                        .and_then(|value| value.as_bool())
                                        .unwrap_or(false);
                                    let artifact = if text.lines().count() > 25 {
                                        Some(Artifact {
                                            stream_revision: None,
                                            id: format!("art-{}", Uuid::new_v4()),
                                            title: format!("MCP: {}", prep.function_name),
                                            artifact_type: "markdown".into(),
                                            language: Some("markdown".into()),
                                            content: text.clone(),
                                            file_path: None,
                                            timestamp: chrono::Utc::now().timestamp_millis(),
                                        })
                                    } else {
                                        None
                                    };
                                    super::tools::ToolExecutionResult {
                                        output: text,
                                        artifact,
                                        success: !is_error,
                                        cancelled: false,
                                        ..Default::default()
                                    }
                                }
                                Err(error) => super::tools::ToolExecutionResult {
                                    output: error,
                                    artifact: None,
                                    success: false,
                                    cancelled: false,
                                    ..Default::default()
                                },
                            }
                        }
                    } else if is_review_skill_tool(&prep.function_name)
                        || prep.function_name == "learn_skill"
                    {
                        let result = foreground_skill_tools
                            .execute(&prep.function_name, &prep.args)
                            .await;
                        if matches!(prep.function_name.as_str(), "skill_manage" | "learn_skill")
                            && result.success
                        {
                            *skills_shared.write().await = load_skills(&all_skill_paths).await;
                        }
                        result
                    } else if prep.function_name == "use_skill" {
                        let skill_name = prep
                            .args
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        let mut skills_guard = skills_shared.write().await;
                        let activation =
                            activate_skill(&mut skills_guard, skill_name, &all_skill_paths).await;
                        if let Some(skill) = activation.skill {
                            emitter.skill(skill);
                        }
                        activation.result
                    } else if prep.function_name == "spawn_subagent" {
                        let role = prep
                            .args
                            .get("role")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Specialist")
                            .to_string();
                        let task = prep
                            .args
                            .get("task")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let subpath = prep
                            .args
                            .get("subpath")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let params = SubagentParams {
                            role: &role,
                            task: &task,
                            subpath: subpath.as_deref(),
                            skill_paths: &all_skill_paths,
                            inherited_policy: &inherited_policy,
                            execution_mode,
                            completion_options: &completion_options,
                        };
                        run_subagent(
                            &emitter,
                            &request_provider,
                            &workspace_cwd,
                            params,
                            cancel.clone(),
                        )
                        .await
                    } else {
                        let exec = execute_tool(&prep.function_name, &prep.args, &context).await;
                        if prep.function_name == "read_file" {
                            if let Some(file_path) = prep
                                .args
                                .get("filePath")
                                .or_else(|| prep.args.get("file_path"))
                                .and_then(|v| v.as_str())
                            {
                                if file_path.ends_with("SKILL.md") {
                                    let skills_guard = skills_shared.read().await;
                                    for s in skills_guard.iter() {
                                        if file_path.contains(&s.name)
                                            || s.path.ends_with(file_path)
                                        {
                                            emitter.skill(s.clone());
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        exec
                    };

                    let mut artifact_id = None;
                    if let Some(artifact) = execution.artifact.take() {
                        artifact_id = Some(artifact.id.clone());
                        emitter.artifact(artifact);
                    }
                    let completed_at = chrono::Utc::now().timestamp_millis();
                    let duration_ms = execution
                        .duration_ms
                        .or_else(|| Some((completed_at - started_at).max(0) as u64));
                    let status = if execution.cancelled || cancel.is_cancelled() {
                        "cancelled"
                    } else if execution.success {
                        "done"
                    } else {
                        "error"
                    };
                    let tool_item = ToolCallItem {
                        id: prep.ui_id,
                        name: prep.function_name,
                        args: prep.args,
                        status: status.into(),
                        result: Some(execution.output.clone()),
                        artifact_id,
                        arguments_raw: Some(prep.arguments_raw),
                        started_at: Some(started_at),
                        completed_at: Some(completed_at),
                        duration_ms,
                        exit_code: execution.exit_code,
                        terminal_output: execution.terminal_output.clone(),
                        files_changed: execution.files_changed.clone(),
                    };
                    emitter.tool(tool_item);

                    (prep.call_id, execution)
                }
            }, |prep| cancelled_call(prep, &emitter)).await;
            for (call_id, execution) in executed_results {
                conversation.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": execution.output
                }));
            }
            emitter.provider_state(&conversation[current_run_start..]);

            if cancel.is_cancelled() {
                break;
            }
        }
        if !finished && !cancel.is_cancelled() && turns >= request.max_turns.clamp(1, 100) {
            emitter.error(format!(
                "Agent reached the configured maximum of {} turns",
                request.max_turns
            ));
        }
        mcp.shutdown().await;
        let foreground_event = foreground_skill_tools.take_event().await;
        // Reserve while the foreground is still active, before done can trigger
        // completion/finish. The spawned worker owns readiness through its exit.
        let finalization = foreground_event.as_ref().map(|_| self.reserve_finalization());
        emitter.done(cancel.is_cancelled(), finished && !cancel.is_cancelled());
        if let Some(event) = foreground_event.clone() {
            // Native persistence must not hold up foreground completion. A source
            // assistant may not have been saved yet; retry only that race.
            // Publish only after the canonical commit so the UI sees its revision.
            let app = emitter.app.clone();
            tokio::spawn(async move {
                let _finalization = finalization;
                for attempt in 0..5 {
                    match super::storage::record_self_improvement(&event).await {
                        Ok(()) => {
                            let _ = app.emit("agent:self-improvement", &event);
                            break;
                        }
                        Err(error)
                            if attempt < 4
                                && (error
                                    == "The reviewed turn no longer exists in this session"
                                    || error.ends_with("was not found")) =>
                        {
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await
                        }
                        Err(error) => {
                            // Skill changes/pending proposals remain durable, but
                            // must not be advertised as attached to session history.
                            eprintln!(
                                "Review changes for run {} in session {} could not be attached: {error}",
                                event.run_id, event.session_id
                            );
                            break;
                        }
                    }
                }
            });
        }
        // Consume the threshold even when disabled or this run is not reviewable.
        let review_due = self
            .review_cadence
            .lock()
            .map(|mut cadence| {
                cadence.take_due(
                    &request.session_id,
                    request.review_config.creation_nudge_interval,
                )
            })
            .unwrap_or(false);
        if review_due
            && foreground_event.is_none()
            && final_response_nonempty
            && finished
            && !cancel.is_cancelled()
            && request.self_improvement_enabled
        {
            // Reserve synchronously: a newly started foreground run can cancel
            // the lease before the background task is ever polled.
            if let Ok(lease) =
                self.begin_review(&request.run_id, &request.session_id, Some(&cancel))
            {
                let app = emitter.app.clone();
                let context = ReflectionContext {
                    cwd: workspace_cwd,
                    provider: request.provider,
                    snapshot: conversation.clone(),
                    run_id: request.run_id,
                    session_id: request.session_id,
                    config: request.review_config,
                    skill_paths: request.skill_paths,
                    parent_tools: Some(tools.clone()),
                    parent_options: Some(options.clone()),
                };
                tokio::spawn(async move {
                    let _ = run_reflection(app, lease, context).await;
                });
            }
        }
    }

    pub async fn run_council(
        &self,
        app: AgentHost,
        workspace_cwd: String,
        request_a: AgentRunRequest,
        request_b: AgentRunRequest,
        cancel: CancellationToken,
    ) {
        let emitter = EmitterContext {
            app: app.clone(),
            run_id: request_a.run_id.clone(),
            session_id: request_a.session_id.clone(),
        };
        if let Err(error) =
            super::images::validate_request_images(&request_a.messages, &request_a.provider)
                .and_then(|_| {
                    super::images::validate_request_images(&request_b.messages, &request_b.provider)
                })
        {
            emitter.error(error);
            emitter.done(false, false);
            return;
        }

        let selected_context = async {
            let mut names_a = selected_skill_names(&request_a.messages)?;
            let mut names_b = selected_skill_names(&request_b.messages)?;
            names_a.sort();
            names_b.sort();
            if names_a != names_b {
                return Err("Council requests must use the same selected skills.".into());
            }
            let paths = effective_skill_paths(
                &workspace_cwd, &request_a.skill_paths, &request_a.review_config,
            )?;
            let skills = load_skills(&paths).await;
            load_selected_skill_context(&request_a.messages, &skills, &emitter).await
        }.await;
        let selected_context = match selected_context {
            Ok(context) => context,
            Err(error) => {
                emitter.error(error);
                emitter.done(false, false);
                return;
            }
        };

        let default_prompt = "You are AetherAgent, an expert developer assistant. Follow these strict guidelines:\n- Maintain a clean, minimalist developer aesthetic.\n- Never use emojis, emotes, smileys, decorative symbols, or gamified AI tropes in responses.\n- Provide a direct, comprehensive, and high-quality response.\n- When referencing or creating generated files (documents, scripts, reports, images, etc.), always provide a clickable Markdown link with the exact absolute file path (e.g. `[filename.ext](file:///D:/path/to/filename.ext)` or `[filename.ext](D:/path/to/filename.ext)`), so the user can click it to open the file directly in their operating system.";
        let mut system_a = match request_a
            .system_prompt
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            Some(custom) => format!("{default_prompt}\n\n## Operator instructions\n{custom}"),
            None => default_prompt.to_string(),
        };

        let mut system_b = match request_b
            .system_prompt
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            Some(custom) => format!("{default_prompt}\n\n## Operator instructions\n{custom}"),
            None => default_prompt.to_string(),
        };
        if !workspace_cwd.is_empty() {
            let workspace_section = format!("\n## Active workspace\n{workspace_cwd}");
            system_a.push_str(&workspace_section);
            system_b.push_str(&workspace_section);
        }
        let mode_instructions = match request_a.execution_mode {
            ExecutionMode::RequestReview => "\n## Permission & Execution Mode: REQUEST REVIEW\nEvery command and destructive operation requires explicit operator approval before execution. You must ask the user for confirmation and explain what the command does before proceeding with write or execute tools.",
            ExecutionMode::Default => "\n## Permission & Execution Mode: DEFAULT\nAsk before sensitive or destructive operations (e.g. deleting files, rm/Remove-Item, git reset --hard, force push, drop table, kill process). Standard non-destructive reads and edits can proceed directly.",
            ExecutionMode::Auto => "\n## Permission & Execution Mode: AUTO (PREVIEW)\nAutomatically evaluate the impact of operations. Proceed autonomously on routine actions, but pause and request operator confirmation if an operation has significant risk of data loss.",
            ExecutionMode::Turbo => "\n## Permission & Execution Mode: TURBO\nRun all commands, tool calls, and workflows autonomously without restrictions or confirmation prompts unless explicitly requested by the operator.",
        };
        system_a.push_str(LEAN_WORKFLOW);
        system_b.push_str(LEAN_WORKFLOW);
        system_a.push_str(mode_instructions);
        system_b.push_str(mode_instructions);
        match load_workspace_memory(&workspace_cwd).await {
            Ok(Some(memory)) => {
                let section = format!("\n## Workspace AGENTS.md instructions\n{memory}");
                system_a.push_str(&section);
                system_b.push_str(&section);
            }
            Ok(None) => {}
            Err(error) => emitter.error(error),
        }

        let mut req_a_messages = messages_with_skill_context(&request_a.messages, &selected_context);
        if request_a.auto_compaction_enabled {
            let threshold = request_a.auto_compaction_threshold_tokens.unwrap_or(65_536);
            let _ = apply_micro_compaction(
                &mut req_a_messages,
                &request_a.run_id,
                &request_a.session_id,
                threshold,
            );
        }
        let mut conv_a = Vec::new();
        conv_a.push(serde_json::json!({"role": "system", "content": system_a}));
        for message in &req_a_messages {
            append_history_message(&mut conv_a, message);
        }

        let mut req_b_messages = messages_with_skill_context(&request_b.messages, &selected_context);
        if request_b.auto_compaction_enabled {
            let threshold = request_b.auto_compaction_threshold_tokens.unwrap_or(65_536);
            let _ = apply_micro_compaction(
                &mut req_b_messages,
                &request_b.run_id,
                &request_b.session_id,
                threshold,
            );
        }
        let mut conv_b = Vec::new();
        conv_b.push(serde_json::json!({"role": "system", "content": system_b}));
        for message in &req_b_messages {
            append_history_message(&mut conv_b, message);
        }

        let empty_tools_a = serde_json::Value::Array(vec![]);
        let empty_tools_b = serde_json::Value::Array(vec![]);

        let options_a = CompletionOptions {
            thinking: request_a.thinking,
            reasoning_effort: request_a.reasoning_effort,
            temperature: request_a.temperature,
            max_tokens: request_a.max_tokens,
            stream: request_a.stream_response,
        };

        let options_b = CompletionOptions {
            thinking: request_b.thinking,
            reasoning_effort: request_b.reasoning_effort,
            temperature: request_b.temperature,
            max_tokens: request_b.max_tokens,
            stream: request_b.stream_response,
        };

        let council_success = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let success_a = council_success.clone();
        let success_b = council_success.clone();
        let start_a = std::time::Instant::now();
        let council_error_a = emitter.clone();
        let app_a = app.clone();
        let run_id_a = request_a.run_id.clone();
        let session_id_a = request_a.session_id.clone();
        let cancel_a = cancel.clone();

        let task_a = async move {
            if cancel_a.is_cancelled() {
                return;
            }
            let app_clone = app_a.clone();
            let r_id = run_id_a.clone();
            let s_id = session_id_a.clone();
            let app_clone2 = app_a.clone();
            let r_id2 = run_id_a.clone();
            let s_id2 = session_id_a.clone();

            let result_a = stream_chat_completion(
                &request_a.provider,
                &serde_json::Value::Array(conv_a),
                &empty_tools_a,
                &options_a,
                move |chunk| {
                    let _ = app_clone.emit(
                        "agent-event",
                        AgentEvent::CouncilChunk(CouncilChunkData {
                            run_id: r_id.clone(),
                            session_id: s_id.clone(),
                            candidate: "A".into(),
                            kind: "content".into(),
                            text: chunk,
                        }),
                    );
                },
                move |reasoning| {
                    let _ = app_clone2.emit(
                        "agent-event",
                        AgentEvent::CouncilChunk(CouncilChunkData {
                            run_id: r_id2.clone(),
                            session_id: s_id2.clone(),
                            candidate: "A".into(),
                            kind: "reasoning".into(),
                            text: reasoning,
                        }),
                    );
                },
                |_| {},
                cancel_a.clone(),
            )
            .await;
            if let Err(error) = result_a {
                success_a.store(false, std::sync::atomic::Ordering::Relaxed);
                council_error_a.error(redact_provider_error(error, &request_a.provider));
            }

            let duration_ms = start_a.elapsed().as_millis() as i64;
            let _ = app_a.emit(
                "agent-event",
                AgentEvent::CouncilDone(CouncilDoneData {
                    run_id: run_id_a,
                    session_id: session_id_a,
                    candidate: "A".into(),
                    duration_ms,
                }),
            );
        };

        let start_b = std::time::Instant::now();
        let council_error_b = emitter.clone();
        let app_b = app.clone();
        let run_id_b = request_b.run_id.clone();
        let session_id_b = request_b.session_id.clone();
        let cancel_b = cancel.clone();

        let task_b = async move {
            if cancel_b.is_cancelled() {
                return;
            }
            let app_clone = app_b.clone();
            let r_id = run_id_b.clone();
            let s_id = session_id_b.clone();
            let app_clone2 = app_b.clone();
            let r_id2 = run_id_b.clone();
            let s_id2 = session_id_b.clone();

            let result_b = stream_chat_completion(
                &request_b.provider,
                &serde_json::Value::Array(conv_b),
                &empty_tools_b,
                &options_b,
                move |chunk| {
                    let _ = app_clone.emit(
                        "agent-event",
                        AgentEvent::CouncilChunk(CouncilChunkData {
                            run_id: r_id.clone(),
                            session_id: s_id.clone(),
                            candidate: "B".into(),
                            kind: "content".into(),
                            text: chunk,
                        }),
                    );
                },
                move |reasoning| {
                    let _ = app_clone2.emit(
                        "agent-event",
                        AgentEvent::CouncilChunk(CouncilChunkData {
                            run_id: r_id2.clone(),
                            session_id: s_id2.clone(),
                            candidate: "B".into(),
                            kind: "reasoning".into(),
                            text: reasoning,
                        }),
                    );
                },
                |_| {},
                cancel_b.clone(),
            )
            .await;
            if let Err(error) = result_b {
                success_b.store(false, std::sync::atomic::Ordering::Relaxed);
                council_error_b.error(redact_provider_error(error, &request_b.provider));
            }

            let duration_ms = start_b.elapsed().as_millis() as i64;
            let _ = app_b.emit(
                "agent-event",
                AgentEvent::CouncilDone(CouncilDoneData {
                    run_id: run_id_b,
                    session_id: session_id_b,
                    candidate: "B".into(),
                    duration_ms,
                }),
            );
        };

        tokio::join!(task_a, task_b);
        emitter.done(
            cancel.is_cancelled(),
            !cancel.is_cancelled() && council_success.load(std::sync::atomic::Ordering::Relaxed),
        );
    }
}

struct ReflectionLease {
    // Fields drop after Drop::drop releases the registry lock; never acquire the
    // gate while holding reviews (admission takes these locks in reverse order).
    _work: NativeWorkLease,
    registry: Arc<Mutex<HashMap<String, ReviewState>>>,
    session_id: String,
    id: String,
    cancel: CancellationToken,
}

impl ReflectionLease {
    fn set_run_id(&self, run_id: &str) -> Result<(), String> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| "Review registry is unavailable")?;
        let state = registry
            .get_mut(&self.session_id)
            .filter(|state| state.id == self.id && !self.cancel.is_cancelled())
            .ok_or("Review was cancelled")?;
        state.run_id = run_id.to_string();
        Ok(())
    }
}

impl Drop for ReflectionLease {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.registry.lock() {
            if registry
                .get(&self.session_id)
                .is_some_and(|state| state.id == self.id)
            {
                registry.remove(&self.session_id);
            }
        }
    }
}

struct ReflectionContext {
    cwd: String,
    provider: ProviderConfig,
    snapshot: Vec<serde_json::Value>,
    run_id: String,
    session_id: String,
    config: ReviewConfig,
    skill_paths: Vec<String>,
    parent_tools: Option<serde_json::Value>,
    parent_options: Option<CompletionOptions>,
}

fn review_provider(
    parent: &ProviderConfig,
    config: &ReviewConfig,
    providers: &[ProviderConfig],
) -> Result<(ProviderConfig, bool), String> {
    match config
        .auxiliary_provider_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        Some(id) if id != parent.id => providers
            .iter()
            .find(|provider| provider.id == id)
            .cloned()
            .map(|provider| (provider, true))
            .ok_or_else(|| "The auxiliary review provider is unavailable".to_string()),
        _ => Ok((parent.clone(), false)),
    }
}

async fn run_reflection(
    app: AgentHost,
    lease: ReflectionLease,
    mut context: ReflectionContext,
) -> Result<(), String> {
    let outcome = async {
        if lease.cancel.is_cancelled() {
            return Err("Review was cancelled".to_string());
        }
        if context
            .config
            .auxiliary_provider_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty() && id != context.provider.id)
        {
            let settings = super::storage::load_settings().await?;
            let (provider, routed) =
                review_provider(&context.provider, &context.config, &settings.providers)?;
            context.provider = provider;
            if routed {
                context.parent_tools = None;
                context.parent_options = None;
            }
        }
        let mut event = super::reflection::review(super::reflection::ReviewRequest {
            workspace_cwd: &context.cwd,
            provider: &context.provider,
            conversation: &context.snapshot,
            run_id: &context.run_id,
            session_id: &context.session_id,
            cancel: lease.cancel.clone(),
            config: &context.config,
            skill_paths: &context.skill_paths,
            parent_tools: context.parent_tools.as_ref(),
            parent_options: context.parent_options.as_ref(),
        })
        .await?;
        if lease.cancel.is_cancelled() {
            if event.changes.is_empty() {
                return Err("Review was cancelled".to_string());
            }
            // Writes already happened: persist and emit their actual results.
            let summary = event.summary.get_or_insert_with(String::new);
            if !summary.to_ascii_lowercase().contains("cancel") {
                if !summary.is_empty() {
                    summary.push_str("; ");
                }
                summary.push_str("Review was cancelled after partial changes");
            }
        }
        event.workspace_cwd = Some(context.cwd.clone());
        super::storage::record_self_improvement(&event).await?;
        app.emit("agent:self-improvement", &event)
            .map_err(|error| error.to_string())
    }
    .await;
    if let Err(error) = outcome {
        let message = redact_provider_error(error, &context.provider);
        let _ = app.emit(
            "agent:self-improvement-error",
            serde_json::json!({"run_id":context.run_id,"session_id":context.session_id,"message":message}),
        );
        return Err(message);
    }
    Ok(())
}

pub fn completed_review_snapshot(
    messages: &[ChatMessage],
) -> Result<(String, Vec<serde_json::Value>), String> {
    let last = messages
        .last()
        .ok_or("Complete a conversational turn before reviewing")?;
    if last.role != "assistant"
        || last.is_streaming == Some(true)
        || (last.run_id.is_some() && last.run_status.as_deref() != Some("completed"))
        || last
            .run_status
            .as_deref()
            .is_some_and(|status| status != "completed")
        || last.tool_calls.as_ref().is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| matches!(tool.status.as_str(), "pending" | "running" | "cancelled"))
        })
        || (last.content.trim().is_empty() && last.tool_calls.as_ref().is_none_or(Vec::is_empty))
    {
        return Err("Only a successfully completed conversational turn can be reviewed".into());
    }
    if !messages.iter().any(|message| message.role == "user") {
        return Err("No user turn is available to review".into());
    }
    let mut snapshot = Vec::new();
    for message in messages {
        append_history_message(&mut snapshot, message);
    }
    Ok((
        last.run_id.clone().unwrap_or_else(|| last.id.clone()),
        snapshot,
    ))
}

pub fn estimate_messages_tokens(messages: &[ChatMessage], system: &str) -> usize {
    let mut chars = system.len();
    let mut image_tokens = 0;
    for message in messages {
        // Encoded bytes are not text tokens; budget conservatively per image.
        image_tokens += message.image_inputs.as_deref().unwrap_or(&[]).len() * 1600;
        chars += message.content.len();
        if let Some(reasoning) = &message.reasoning {
            chars += reasoning.len();
        }
        if let Some(tool_calls) = &message.tool_calls {
            for call in tool_calls {
                chars += call.name.len();
                chars += call.args.to_string().len();
                if let Some(result) = &call.result {
                    chars += result.len();
                }
            }
        }
    }
    chars.div_ceil(4) + image_tokens
}

pub fn apply_micro_compaction(
    messages: &mut [ChatMessage],
    run_id: &str,
    session_id: &str,
    threshold_tokens: u32,
) -> Option<MicroCompactionEvent> {
    let current_tokens = estimate_messages_tokens(messages, "");
    if current_tokens <= threshold_tokens as usize {
        return None;
    }

    let user_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user")
        .map(|(i, _)| i)
        .collect();

    if user_indices.len() < 2 {
        return None;
    }

    let cutoff = user_indices[user_indices.len() - 2];
    let mut pruned_count = 0;
    let mut saved_chars = 0;

    for msg in &mut messages[..cutoff] {
        let previous_count = pruned_count;
        let mut replacements = Vec::new();
        if msg.role == "assistant" {
            if let Some(tool_calls) = &mut msg.tool_calls {
                for call in tool_calls.iter_mut() {
                    if let Some(res) = &call.result {
                        let lines = res.lines().count();
                        let chars = res.len();
                        if (lines > 35 || chars > 1000) && !res.starts_with("[Output pruned:") {
                            let replacement = format!(
                                "[Output pruned: {} lines ({} chars) from '{}'. Full output preserved in session history]",
                                lines, chars, call.name
                            );
                            saved_chars += chars.saturating_sub(replacement.len());
                            replacements.push((res.clone(), replacement.clone()));
                            call.result = Some(replacement);
                            pruned_count += 1;
                        }
                    }
                }
            }
            if pruned_count > previous_count {
                // Canonical segments must reflect the compacted aggregate too.
                if let (Some(segments), Some(calls)) = (&mut msg.segments, &msg.tool_calls) {
                    for segment in segments {
                        if let Some(items) = &mut segment.tool_calls {
                            for item in items {
                                if let Some(compacted) =
                                    calls.iter().find(|call| call.id == item.id)
                                {
                                    item.result = compacted.result.clone();
                                }
                            }
                        }
                    }
                }
                // Only tool output is pruned; signed assistant blocks remain
                // byte-for-byte unchanged across compaction and replay.
                if let Some(native) = &mut msg.provider_messages {
                    for message in native
                        .iter_mut()
                        .filter(|message| message["role"] == "tool")
                    {
                        if let Some((_, replacement)) = replacements.iter().find(|(original, _)| {
                            message["content"].as_str() == Some(original.as_str())
                        }) {
                            message["content"] = serde_json::json!(replacement);
                        }
                    }
                }
            }
        }
    }

    if pruned_count > 0 {
        Some(MicroCompactionEvent {
            run_id: run_id.to_string(),
            session_id: session_id.to_string(),
            pruned_tool_count: pruned_count,
            estimated_tokens_saved: saved_chars.div_ceil(4),
        })
    } else {
        None
    }
}

fn valid_provider_messages(messages: &[serde_json::Value]) -> bool {
    if messages.is_empty() {
        return false;
    }
    let mut pending = std::collections::HashSet::new();
    let mut seen = std::collections::HashSet::new();
    for message in messages {
        match message.get("role").and_then(|value| value.as_str()) {
            Some("assistant") => {
                if !pending.is_empty() {
                    return false;
                }
                // Native blocks are authoritative. Anthropic's helper deliberately
                // emits no OpenAI tool_calls; signed blocks must remain untouched.
                let mut native_calls = std::collections::HashMap::new();
                let native_content = match message.get("content") {
                    Some(serde_json::Value::String(_) | serde_json::Value::Null) => false,
                    Some(serde_json::Value::Array(blocks)) => {
                        for block in blocks {
                            match block.get("type").and_then(|value| value.as_str()) {
                                Some("text" | "thinking" | "redacted_thinking") => {}
                                Some("tool_use") => {
                                    let Some(id) = block
                                        .get("id")
                                        .and_then(|value| value.as_str())
                                        .filter(|id| !id.trim().is_empty())
                                    else {
                                        return false;
                                    };
                                    let Some(name) = block
                                        .get("name")
                                        .and_then(|value| value.as_str())
                                        .filter(|name| !name.trim().is_empty())
                                    else {
                                        return false;
                                    };
                                    let Some(input) =
                                        block.get("input").filter(|input| input.is_object())
                                    else {
                                        return false;
                                    };
                                    if native_calls.insert(id, (name, input.clone())).is_some() {
                                        return false;
                                    }
                                }
                                _ => return false,
                            }
                        }
                        true
                    }
                    _ => return false,
                };
                let mut calls = std::collections::HashMap::new();
                if let Some(mirrors) = message.get("tool_calls").filter(|value| !value.is_null()) {
                    let Some(mirrors) = mirrors.as_array() else {
                        return false;
                    };
                    for call in mirrors {
                        let Some(id) = call
                            .get("id")
                            .and_then(|value| value.as_str())
                            .filter(|id| !id.trim().is_empty())
                        else {
                            return false;
                        };
                        let Some(function) = call.get("function") else {
                            return false;
                        };
                        let Some(name) = function
                            .get("name")
                            .and_then(|value| value.as_str())
                            .filter(|name| !name.trim().is_empty())
                        else {
                            return false;
                        };
                        let Some(args) = function.get("arguments").and_then(|value| value.as_str())
                        else {
                            return false;
                        };
                        let Ok(input) = serde_json::from_str::<serde_json::Value>(args) else {
                            return false;
                        };
                        if !input.is_object() || calls.insert(id, (name, input)).is_some() {
                            return false;
                        }
                    }
                    // Legacy mirrors may describe the same calls, never add calls
                    // or disagree with native names/inputs. Compare parsed objects.
                    if native_content && calls != native_calls {
                        return false;
                    }
                }
                let authoritative = if native_content { native_calls } else { calls };
                for id in authoritative.keys() {
                    if !seen.insert(id.to_string()) {
                        return false;
                    }
                    pending.insert(id.to_string());
                }
            }
            Some("tool") => {
                let Some(id) = message.get("tool_call_id").and_then(|value| value.as_str()) else {
                    return false;
                };
                if !pending.remove(id)
                    || !message
                        .get("content")
                        .is_some_and(|value| value.is_string())
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
    pending.is_empty()
}

fn append_history_message(conversation: &mut Vec<serde_json::Value>, message: &ChatMessage) {
    if message.role == "assistant" {
        if let Some(native) = message
            .provider_messages
            .as_ref()
            .filter(|messages| valid_provider_messages(messages))
        {
            conversation.extend(native.iter().cloned());
            return;
        }
    }
    match message.role.as_str() {
        "user" => conversation.push(
            serde_json::json!({"role":"user","content":super::images::user_content(message)}),
        ),
        "system" => {
            conversation.push(serde_json::json!({"role":"system","content":message.content}))
        }
        "assistant" => {
            let mut seen = std::collections::HashSet::new();
            // Segments are canonical when populated; aggregate content/toolCalls
            // duplicate their data and must not be replayed a second time.
            if let Some(segments) = message.segments.as_ref().filter(|segments| {
                segments.iter().any(|segment| {
                    segment.text.as_ref().is_some_and(|text| !text.is_empty())
                        || segment
                            .tool_calls
                            .as_ref()
                            .is_some_and(|calls| !calls.is_empty())
                })
            }) {
                for segment in segments {
                    append_history_segment(
                        conversation,
                        segment.text.as_deref().unwrap_or_default(),
                        segment.tool_calls.as_deref().unwrap_or_default(),
                        &mut seen,
                    );
                }
            } else {
                let preamble = message.preamble.as_deref().filter(|text| !text.is_empty());
                append_history_segment(
                    conversation,
                    preamble.unwrap_or(&message.content),
                    message.tool_calls.as_deref().unwrap_or_default(),
                    &mut seen,
                );
                if preamble.is_some() && !message.content.is_empty() {
                    append_history_segment(conversation, &message.content, &[], &mut seen);
                }
            }
        }
        _ => {}
    }
}

fn append_history_segment(
    conversation: &mut Vec<serde_json::Value>,
    text: &str,
    items: &[ToolCallItem],
    seen: &mut std::collections::HashSet<String>,
) {
    let paired: Vec<_> = items
        .iter()
        .filter_map(|item| {
            if item.id.trim().is_empty()
                || item.name.trim().is_empty()
                || !matches!(item.status.as_str(), "done" | "error" | "cancelled")
                || item.result.is_none()
                || seen.contains(&item.id)
            {
                return None;
            }
            let arguments = item
                .arguments_raw
                .as_deref()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    item.args
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| item.args.to_string())
                });
            let parsed = serde_json::from_str::<serde_json::Value>(&arguments).ok()?;
            if !parsed.is_object() {
                return None;
            }
            seen.insert(item.id.clone());
            Some((item, arguments))
        })
        .collect();
    if text.is_empty() && paired.is_empty() {
        return;
    }
    let calls: Vec<_> = paired
        .iter()
        .map(|(item, arguments)| {
            serde_json::json!({
                "id":item.id,"type":"function","function":{"name":item.name,"arguments":arguments}
            })
        })
        .collect();
    conversation.push(serde_json::json!({
        "role":"assistant","content":text,
        "tool_calls":if calls.is_empty(){serde_json::Value::Null}else{serde_json::Value::Array(calls)}
    }));
    for (item, _) in paired {
        conversation
            .push(serde_json::json!({"role":"tool","tool_call_id":item.id,"content":item.result}));
    }
}

fn redact_provider_error(mut error: String, provider: &ProviderConfig) -> String {
    for secret in super::reflection::provider_secrets(provider) {
        if !secret.is_empty() {
            error = error.replace(&secret, "[redacted]");
        }
    }
    error
}

#[cfg(test)]
mod tests {
    use super::super::skills::workspace_skill_paths;
    use super::super::types::{ProviderKind, ReasoningEffort};
    use super::*;

    #[tokio::test]
    async fn selected_skills_reach_latest_provider_user_without_mutating_display_or_images() {
        let (cwd, _) = workspace_with_skill("selected-fixture", "Follow every fixture instruction.");
        let catalog = load_skills(&workspace_skill_paths(&cwd, &[])).await;
        let messages: Vec<ChatMessage> = serde_json::from_value(serde_json::json!([
            {"id":"old", "role":"user", "content":"Old prompt", "timestamp":1,
             "selectedSkills":["removed-historical-skill"]},
            {"id":"answer", "role":"assistant", "content":"Display answer", "timestamp":2,
             "providerMessages":[{"role":"assistant","content":"Native answer"}]},
            {"id":"latest", "role":"user", "content":"Current prompt", "timestamp":3,
             "selectedSkills":["selected-fixture"],
             "imageInputs":[{"sourcePath":"image.png","mediaType":"image/png","data":"aW1hZ2U="}]},
            {"id":"trailing", "role":"assistant", "content":"", "timestamp":4,
             "selectedSkills":["ignored-assistant-selection"]}
        ])).unwrap();
        let original = serde_json::to_value(&messages).unwrap();
        let events = ToolEventCollector::default();
        let emitter = EmitterContext {
            app: AgentHost::new(events.clone()), run_id: "selected-run".into(),
            session_id: "selected-session".into(),
        };
        let context = load_selected_skill_context(&messages, &catalog, &emitter).await.unwrap();
        let augmented = messages_with_skill_context(&messages, &context);
        let mut conversation = vec![serde_json::json!({"role":"system","content":"System unchanged"})];
        for message in &augmented {
            append_history_message(&mut conversation, message);
        }
        assert_eq!(conversation[0]["content"], "System unchanged");
        assert_eq!(conversation[1]["content"], "Old prompt");
        assert_eq!(conversation[2]["content"], "Native answer");
        let blocks = &conversation[3]["content"];
        let text = blocks[0]["text"].as_str().unwrap();
        assert!(text.starts_with("Current prompt"));
        assert!(text.contains("Follow every fixture instruction."), "{text}");
        assert!(text.contains("name: selected-fixture"), "full skill content is required");
        assert_eq!(blocks[1]["type"], "image_url");
        assert_eq!(blocks[1]["image_url"]["url"], "data:image/png;base64,aW1hZ2U=");
        assert_eq!(serde_json::to_value(&messages).unwrap(), original);
        let events = events.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "skill_activated");
        assert_eq!(events[0]["data"]["skill"]["name"], "selected-fixture");
        assert_eq!(events[0]["data"]["runId"], "selected-run");
    }

    fn selected_skill_messages(names: &[&str]) -> Vec<ChatMessage> {
        serde_json::from_value(serde_json::json!([{
            "id":"selected-user", "role":"user", "content":"Prompt", "timestamp":1,
            "selectedSkills":names
        }])).unwrap()
    }

    fn selected_skill_emitter(events: &ToolEventCollector) -> EmitterContext {
        EmitterContext {
            app: AgentHost::new(events.clone()), run_id: "selected-run".into(),
            session_id: "selected-session".into(),
        }
    }

    #[tokio::test]
    async fn selected_skills_deduplicate_case_insensitively_and_reject_invalid_names() {
        let (cwd, _) = workspace_with_skill("selected-fixture", "Unique instruction body.");
        let catalog = load_skills(&workspace_skill_paths(&cwd, &[])).await;
        let events = ToolEventCollector::default();
        let emitter = selected_skill_emitter(&events);
        let context = load_selected_skill_context(
            &selected_skill_messages(&["selected-fixture", "SELECTED-FIXTURE", "Selected-Fixture"]),
            &catalog, &emitter,
        ).await.unwrap();
        assert_eq!(context.matches("Unique instruction body.").count(), 1);
        assert_eq!(events.0.lock().unwrap().len(), 1);
        events.0.lock().unwrap().clear();
        for name in ["", " ", " selected-fixture", "selected-fixture ", "../selected-fixture",
                     "C:\\skills\\SKILL.md", "selected/fixture", "selected\nfixture", "unknown-skill"] {
            assert!(load_selected_skill_context(
                &selected_skill_messages(&["selected-fixture", name]), &catalog, &emitter,
            ).await.is_err(), "must reject {name:?}");
            assert!(events.0.lock().unwrap().is_empty(), "no activation on partial failure");
        }
        assert!(load_selected_skill_context(
            &selected_skill_messages(&["selected-fixture"; 6]), &catalog, &emitter,
        ).await.is_err(), "at most five selected names");
    }

    #[tokio::test]
    async fn selected_skills_bound_aggregate_context_bytes_before_activation() {
        let (cwd, root) = workspace_with_skill("selected-large-a", &"é".repeat(70 * 1024));
        let first = std::fs::read_to_string(
            root.join(".aether/skills/selected-large-a/SKILL.md"),
        ).unwrap();
        let second_dir = root.join(".aether/skills/selected-large-b");
        std::fs::create_dir_all(&second_dir).unwrap();
        let first_header = "\n\n## Selected skill: selected-large-a\n";
        let second_header = "\n\n## Selected skill: selected-large-b\n";
        let metadata = "---\nname: selected-large-b\ndescription: Regression fixture.\n---\n";
        let padding = 256 * 1024 - first_header.len() - first.len()
            - second_header.len() - metadata.len();
        let mut second = format!("{metadata}{}", "x".repeat(padding));
        let path = second_dir.join("SKILL.md");
        std::fs::write(&path, &second).unwrap();
        let catalog = load_skills(&workspace_skill_paths(&cwd, &[])).await;
        let messages = selected_skill_messages(&["selected-large-a", "selected-large-b"]);
        let events = ToolEventCollector::default();
        let emitter = selected_skill_emitter(&events);
        let context = load_selected_skill_context(&messages, &catalog, &emitter).await.unwrap();
        assert_eq!(context.len(), 256 * 1024, "exactly 256 KiB is allowed");
        assert_eq!(events.0.lock().unwrap().len(), 2);
        events.0.lock().unwrap().clear();

        second.push('x');
        std::fs::write(&path, second).unwrap();
        match load_selected_skill_context(&messages, &catalog, &emitter).await {
            Err(error) => assert!(error.contains("256 KiB"), "{error}"),
            Ok(_) => panic!("must reject aggregate context one byte over 256 KiB"),
        }
        assert!(events.0.lock().unwrap().is_empty(), "no activation on partial failure");
    }

    #[tokio::test]
    async fn selected_skills_fail_closed_when_file_is_unreadable_after_discovery() {
        let (cwd, root) = workspace_with_skill("selected-readable", "Readable instructions.");
        let unreadable_dir = root.join(".aether/skills/selected-unreadable");
        std::fs::create_dir_all(&unreadable_dir).unwrap();
        let path = unreadable_dir.join("SKILL.md");
        std::fs::write(&path,
            "---\nname: selected-unreadable\ndescription: Regression fixture.\n---\nUnavailable instructions.\n",
        ).unwrap();
        let catalog = load_skills(&workspace_skill_paths(&cwd, &[])).await;
        assert!(find_skill(&catalog, "selected-unreadable").is_some());
        // Moving the discovered file makes it unreadable without platform-specific ACL changes.
        std::fs::rename(&path, unreadable_dir.join("SKILL.unavailable")).unwrap();
        let events = ToolEventCollector::default();
        let error = load_selected_skill_context(
            &selected_skill_messages(&["selected-readable", "selected-unreadable"]),
            &catalog, &selected_skill_emitter(&events),
        ).await.unwrap_err();
        assert!(error.contains("Could not load selected skill 'selected-unreadable'"), "{error}");
        assert!(events.0.lock().unwrap().is_empty(), "no activation on partial failure");
    }

    fn selected_skill_request(cwd: &str, names: &[&str]) -> AgentRunRequest {
        serde_json::from_value(serde_json::json!({
            "runId":"selected-run", "sessionId":"selected-session",
            "provider":{"baseUrl":"", "model":"must-not-reach-provider"},
            "messages":selected_skill_messages(names), "maxTurns":1,
            "mcpServers":[{"id":"must-not-connect", "name":"must-not-connect",
                "command":std::path::Path::new(cwd).join("must-not-spawn.exe"), "enabled":true}],
            "autoCompactionEnabled":false,
            "reviewConfig":{"createDir":std::path::Path::new(cwd).join("review-skills")}
        })).unwrap()
    }

    #[tokio::test]
    async fn selected_skills_runner_missing_skill_emits_error_done_before_mcp_or_provider() {
        let (cwd, _) = workspace_with_skill("selected-fixture", "Local instructions.");
        let events = ToolEventCollector::default();
        AgentRunner::new().run(
            AgentHost::new(events.clone()), cwd.clone(),
            selected_skill_request(&cwd, &["missing-selected-fixture"]),
            CancellationToken::new(),
        ).await;
        let events = events.0.lock().unwrap();
        assert_eq!(events.len(), 2, "only selection error + done, no MCP warning or provider events: {events:?}");
        assert_eq!(events[0]["type"], "error");
        assert_eq!(events[0]["data"]["message"],
            "Selected skill 'missing-selected-fixture' was not found in the catalog.");
        assert_eq!(events[1]["type"], "done");
        assert_eq!(events[1]["data"]["successful"], false);
        assert_eq!(events[1]["data"]["cancelled"], false);
    }

    #[tokio::test]
    async fn selected_skills_council_validates_both_requests_before_either_provider() {
        let (cwd, _) = workspace_with_skill("selected-fixture", "Shared instructions.");
        for (names_a, names_b, expected) in [
            (vec!["missing-selected-fixture"], vec!["missing-selected-fixture"], "was not found in the catalog"),
            (vec!["../invalid"], vec!["selected-fixture"], "Invalid selected skill name"),
            (vec!["selected-fixture"], vec!["../invalid"], "Invalid selected skill name"),
            (vec!["selected-fixture"], vec!["selected-fixture"; 6], "at most five"),
            (vec!["selected-fixture"], vec![], "same selected skills"),
            (vec![], vec!["selected-fixture"], "same selected skills"),
        ] {
            let events = ToolEventCollector::default();
            AgentRunner::new().run_council(
                AgentHost::new(events.clone()), cwd.clone(),
                selected_skill_request(&cwd, &names_a), selected_skill_request(&cwd, &names_b),
                CancellationToken::new(),
            ).await;
            let events = events.0.lock().unwrap();
            assert_eq!(events.len(), 2, "no provider or partial activation events: {events:?}");
            assert_eq!(events[0]["type"], "error");
            assert!(events[0]["data"]["message"].as_str().unwrap().contains(expected), "{events:?}");
            assert_eq!(events[1]["type"], "done");
            assert_eq!(events[1]["data"]["successful"], false);
            assert_eq!(events[1]["data"]["cancelled"], false);
        }
    }

    #[test]
    fn update_gate_refuses_active_and_cancelled_runs_until_finish() {
        let runner = AgentRunner::new();
        let cancel = runner.begin("run", "session").unwrap();
        assert!(runner.acquire_update_permit().is_err());
        runner.abort("run").unwrap();
        assert!(cancel.is_cancelled());
        assert!(
            runner.acquire_update_permit().is_err(),
            "cancellation is not completion; the registry must drain first"
        );
        runner.finish("run");
        let permit = runner.acquire_update_permit().unwrap();
        assert!(runner.acquire_update_permit().is_err());
        drop(permit);
        assert!(runner.acquire_update_permit().is_ok());
    }

    #[test]
    fn update_gate_blocks_new_runs_until_permit_drops() {
        let runner = AgentRunner::new();
        let permit = runner.acquire_update_permit().unwrap();
        assert!(runner.begin("new-run", "new-session").is_err());
        assert!(runner.active.lock().unwrap().is_empty());
        drop(permit);
        let cancel = runner.begin("new-run", "new-session").unwrap();
        assert!(!cancel.is_cancelled());
        runner.finish("new-run");
        assert!(runner.acquire_update_permit().is_ok());
    }

    #[test]
    fn update_gate_blocks_manual_and_automatic_reviews_until_permit_drops() {
        let runner = AgentRunner::new();
        let foreground = CancellationToken::new();
        let permit = runner.acquire_update_permit().unwrap();
        assert!(runner.begin_review("manual", "manual-session", None).is_err());
        assert!(runner
            .begin_review("automatic", "automatic-session", Some(&foreground))
            .is_err());
        assert!(runner.reviews.lock().unwrap().is_empty());
        drop(permit);
        let manual = runner.begin_review("manual", "manual-session", None).unwrap();
        let automatic = runner
            .begin_review("automatic", "automatic-session", Some(&foreground))
            .unwrap();
        assert!(runner.acquire_update_permit().is_err());
        drop(manual);
        drop(automatic);
        assert!(runner.acquire_update_permit().is_ok());
    }

    #[test]
    fn update_gate_refuses_cancelled_review_until_lease_drops() {
        let runner = AgentRunner::new();
        let lease = runner.begin_review("review", "session", None).unwrap();
        assert!(runner.acquire_update_permit().is_err());
        runner.abort("review").unwrap();
        assert!(lease.cancel.is_cancelled());
        assert!(runner.acquire_update_permit().is_err());
        drop(lease);
        assert!(runner.acquire_update_permit().is_ok());
    }

    #[test]
    fn update_gate_refuses_removed_review_until_lease_drops() {
        let runner = AgentRunner::new();
        let lease = runner.begin_review("review", "session", None).unwrap();
        let foreground = runner.begin("foreground", "session").unwrap();
        assert!(lease.cancel.is_cancelled());
        assert!(!foreground.is_cancelled());
        assert!(runner.reviews.lock().unwrap().is_empty());
        runner.finish("foreground");
        assert!(runner.active.lock().unwrap().is_empty());
        assert!(
            runner.acquire_update_permit().is_err(),
            "a removed review still owns native work until its actual lease finishes"
        );
        // A draining review must not prevent a new foreground run.
        let next = runner.begin("next", "session").unwrap();
        drop(lease);
        assert!(!next.is_cancelled());
        assert!(runner.acquire_update_permit().is_err());
        runner.finish("next");
        assert!(runner.acquire_update_permit().is_ok());
    }

    #[test]
    fn update_gate_concurrent_acquirers_admit_exactly_one_permit() {
        let runner = Arc::new(AgentRunner::new());
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let runner = runner.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let permit = runner.acquire_update_permit();
                    barrier.wait(); // Keep the winner alive until both tried.
                    permit
                })
            })
            .collect();
        barrier.wait();
        barrier.wait();
        let results: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        drop(results);
        assert!(runner.acquire_update_permit().is_ok());
    }

    #[test]
    fn update_gate_racing_run_or_review_never_overlaps_a_permit() {
        for review in [false, true] {
            for _ in 0..32 {
                let runner = Arc::new(AgentRunner::new());
                let barrier = Arc::new(std::sync::Barrier::new(2));
                let worker = {
                    let runner = runner.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        let permit = runner.acquire_update_permit();
                        barrier.wait(); // Do not release the permit before admission.
                        permit
                    })
                };
                barrier.wait();
                let admitted = if review {
                    runner.begin_review("work", "session", None).map(Some)
                } else {
                    runner.begin("work", "session").map(|_| None)
                };
                barrier.wait();
                let permit = worker.join().unwrap();
                assert_ne!(permit.is_ok(), admitted.is_ok(), "review={review}");
                drop(permit);
                drop(admitted);
                runner.finish("work");
                assert!(runner.acquire_update_permit().is_ok());
            }
        }
    }

    #[tokio::test]
    async fn update_gate_waits_for_detached_finalization_after_foreground_finishes() {
        let runner = AgentRunner::new();
        runner.begin("finalize-run", "finalize-session").unwrap();
        let lease = runner.reserve_finalization();
        let (finish, wait) = tokio::sync::oneshot::channel::<()>();
        let worker = tokio::spawn(async move {
            let _lease = lease;
            let _ = wait.await;
        });
        runner.finish("finalize-run");
        assert!(runner.acquire_update_permit().is_err());
        finish.send(()).unwrap();
        worker.await.unwrap();
        assert!(runner.acquire_update_permit().is_ok());
    }

    #[test]
    fn foreground_finalization_is_reserved_before_done_and_held_by_worker() {
        let source = include_str!("runner.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let start = source
            .find("let foreground_event = foreground_skill_tools.take_event().await;")
            .unwrap();
        let finalization =
            &source[start..source[start..].find("// Consume the threshold").unwrap() + start];
        let reserve = finalization
            .find("self.reserve_finalization()")
            .expect("reserve native persistence before publishing foreground completion");
        let done = finalization.find("emitter.done(").unwrap();
        let worker = finalization.find("tokio::spawn(async move").unwrap();
        let hold = finalization
            .find("let _finalization = finalization;")
            .expect("the detached worker must own the reservation through retries and emission");
        let record = finalization
            .find("record_self_improvement(&event).await")
            .unwrap();
        assert!(reserve < done && done < worker && worker < hold && hold < record);
        assert!(
            !finalization[hold..].contains("drop(_finalization)"),
            "do not release readiness before the worker actually finishes"
        );
    }

    #[test]
    fn foreground_review_event_follows_successful_native_commit() {
        let source = include_str!("runner.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let start = source
            .find("let foreground_event = foreground_skill_tools.take_event().await;")
            .unwrap();
        let finalization =
            &source[start..source[start..].find("// Consume the threshold").unwrap() + start];
        let done = finalization.find("emitter.done(").unwrap();
        let worker = finalization.find("tokio::spawn(async move").unwrap();
        let record = finalization
            .find("record_self_improvement(&event).await")
            .unwrap();
        let success = finalization[record..]
            .find("Ok(()) => {")
            .map(|offset| record + offset);
        let emitted = finalization.find("agent:self-improvement").unwrap();
        assert!(
            done < worker && worker < record && record < emitted,
            "foreground completion must precede a detached commit, then the review event"
        );
        let success = success.expect("only successful commits may emit the review event");
        let next_error = finalization[success..].find("Err(").unwrap() + success;
        assert!(
            success < emitted && emitted < next_error,
            "the review event must occur only inside the successful commit branch"
        );
        assert_eq!(finalization.matches("agent:self-improvement").count(), 1);
        assert!(
            finalization[next_error..].contains("eprintln!"),
            "failed attachment must be logged, not reported as persisted"
        );
    }
    #[tokio::test]
    async fn foreground_changes_attach_to_origin_for_direct_and_staged_writes() {
        for approval in [false, true] {
            let (cwd, root) = workspace_with_skill("event-fixture", "Existing recipe.");
            let created = root.join("configured-skills");
            let config = ReviewConfig {
                create_dir: Some(created.to_string_lossy().into_owned()),
                write_approval: approval,
                ..Default::default()
            };
            let paths = super::super::skills::effective_skill_paths(&cwd, &[], &config).unwrap();
            let tools =
                ForegroundSkillTools::new(&cwd, &paths, &config, "origin-run", "origin-session");
            *tools.store.lock().await = Some(
                super::super::review_skills::ReviewSkillStore::new(
                    &cwd,
                    &paths,
                    created.clone(),
                    root.join("pending"),
                    approval,
                    "origin-run",
                    "origin-session",
                )
                .await
                .unwrap(),
            );
            assert!(tools.take_event().await.is_none());
            let result = tools.execute("learn_skill", &serde_json::json!({"name":"event-created", "description":"Event recipe", "instructions":"Use these verified steps."})).await;
            assert!(result.success, "{}", result.output);
            let event = tools.clone().take_event().await.unwrap();
            assert_eq!(event.run_id, "origin-run");
            assert_eq!(event.session_id, "origin-session");
            assert_eq!(event.workspace_cwd.as_deref(), Some(cwd.as_str()));
            assert_eq!(event.changes.len(), 1);
            assert_eq!(
                event.action,
                if approval {
                    super::super::types::SelfImprovementAction::SkillStaged
                } else {
                    super::super::types::SelfImprovementAction::SkillCreated
                }
            );
            assert_eq!(event.skill_name.as_deref(), Some("event-created"));
            assert!(event
                .summary
                .as_ref()
                .is_some_and(|summary| !summary.is_empty()));
            assert_eq!(event.changes[0].pending_id.is_some(), approval);
            assert!(
                tools.take_event().await.is_none(),
                "changes must be drained across clones"
            );
            let mut skills = load_skills(&paths).await;
            let activation = activate_skill(&mut skills, "event-created", &paths).await;
            assert_eq!(
                activation.result.success, !approval,
                "only applied skills enter the catalog"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn foreground_main_routes_legacy_and_custom_roots_before_native_dispatch() {
        fn normalize(source: &str) -> String {
            source.split_whitespace().collect()
        }

        let source = normalize(
            include_str!("runner.rs")
                .split("#[cfg(test)]")
                .next()
                .unwrap(),
        );
        let routing_branch = normalize(
            r#"else if is_review_skill_tool(&prep.function_name)
                || prep.function_name == "learn_skill"
            {
                let result = foreground_skill_tools
                    .execute(&prep.function_name, &prep.args)
                    .await;"#,
        );
        let routing = source
            .find(&routing_branch)
            .expect("review and legacy learning tools must use foreground dispatch");
        let native = source
            .find(&normalize(
                "execute_tool(&prep.function_name, &prep.args, &context).await",
            ))
            .expect("native tool dispatch must remain present");
        assert!(
            routing < native,
            "foreground routing must precede native dispatch"
        );
        assert!([
            "effective_skill_paths(&workspace_cwd, &request.skill_paths, &request.review_config)",
            "effective_skill_paths(&workspace_cwd, &request.skill_paths, &request.review_config,)",
        ]
        .iter()
        .any(|pattern| source.contains(&normalize(pattern))));
        assert!(source.contains(&normalize("skill_paths: &all_skill_paths")));
        let shutdown = source.rfind("mcp.shutdown().await;").unwrap();
        let finalization = &source[shutdown..];
        assert!(
            finalization.find("take_event().await").unwrap()
                < finalization.find("emitter.done(").unwrap()
        );
        assert!(
            finalization.find("emitter.done(").unwrap()
                < finalization.find("agent:self-improvement").unwrap()
        );
        assert!(
            finalization.contains("foreground_event.is_none()"),
            "a foreground event must not be overwritten by automatic review"
        );
    }
    #[tokio::test]
    async fn legacy_learning_uses_configured_store_and_stages_for_approval() {
        let (cwd, root) = workspace_with_skill("legacy-fixture", "Original instructions.");
        let created = root.join("custom-skills");
        let pending = root.join("pending");
        let tools = ForegroundSkillTools::new(
            &cwd,
            &[],
            &ReviewConfig {
                create_dir: Some(created.to_string_lossy().into_owned()),
                write_approval: true,
                ..Default::default()
            },
            "legacy-run",
            "legacy-session",
        );
        *tools.store.lock().await = Some(
            super::super::review_skills::ReviewSkillStore::new(
                &cwd,
                &[],
                created.clone(),
                pending.clone(),
                true,
                "legacy-run",
                "legacy-session",
            )
            .await
            .unwrap(),
        );
        let args = serde_json::json!({"name":"legacy-created", "description":"A tested recipe", "instructions":"Follow the tested steps."});
        let result = tools.execute("learn_skill", &args).await;
        assert!(result.success, "{}", result.output);
        assert!(result.output.contains("staged"), "{}", result.output);
        assert!(!created.join("legacy-created/SKILL.md").exists());
        let proposals = super::super::review_pending::list_pending(&pending, "legacy-session")
            .await
            .unwrap();
        assert_eq!(proposals.len(), 1);
        assert!(proposals[0]["target_path"]
            .as_str()
            .unwrap_or_else(|| proposals[0]["file_path"].as_str().unwrap())
            .contains("legacy-created"));
        let overwrite = tools
            .execute("learn_skill", &{
                let mut args = args.clone();
                args["overwrite"] = serde_json::json!(true);
                args
            })
            .await;
        assert!(!overwrite.success);
        assert!(
            overwrite.output.contains("skill_view") && overwrite.output.contains("skill_manage"),
            "{}",
            overwrite.output
        );
        assert_eq!(
            super::super::review_pending::list_pending(&pending, "legacy-session")
                .await
                .unwrap()
                .len(),
            1
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn foreground_skill_tools_retain_reads_across_calls_and_initialize_lazily() {
        let (cwd, root) = workspace_with_skill("foreground-fixture", "Keep the old recipe.");
        let created = root.join("created");
        let tools = ForegroundSkillTools::new(
            &cwd,
            &[],
            &ReviewConfig {
                create_dir: Some(created.to_string_lossy().into_owned()),
                ..Default::default()
            },
            "run",
            "session",
        );
        assert!(tools.store.lock().await.is_none());
        assert!(!created.exists());
        let patch = serde_json::json!({"action":"patch","name":"foreground-fixture","old_string":"old recipe","new_string":"tested recipe"});
        let unread = tools.execute("skill_manage", &patch).await;
        assert!(!unread.success);
        assert!(
            unread.output.contains("Read the exact file"),
            "{}",
            unread.output
        );
        let viewed = tools
            .execute(
                "skill_view",
                &serde_json::json!({"name":"foreground-fixture"}),
            )
            .await;
        assert!(viewed.success, "{}", viewed.output);
        let patched = tools.clone().execute("skill_manage", &patch).await;
        assert!(patched.success, "{}", patched.output);
        let viewed = tools
            .execute(
                "skill_view",
                &serde_json::json!({"name":"foreground-fixture"}),
            )
            .await;
        assert!(viewed.output.contains("tested recipe"), "{}", viewed.output);
        let listed = tools.execute("skills_list", &serde_json::json!({})).await;
        assert!(listed.success && listed.output.contains("foreground-fixture"));
        assert!(
            !created.exists(),
            "reading/patching an existing skill must not create the creation root"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn foreground_skill_tools_request_review_allows_only_reads() {
        for name in ["skills_list", "skill_view"] {
            assert!(
                tool_policy_error(
                    ExecutionMode::RequestReview,
                    name,
                    &serde_json::json!({}),
                    false
                )
                .is_none(),
                "{name}"
            );
        }
        assert!(tool_policy_error(
            ExecutionMode::RequestReview,
            "skill_manage",
            &serde_json::json!({}),
            false
        )
        .is_some());
    }

    #[tokio::test]
    async fn foreground_skill_tools_are_explicitly_denied_in_subagents() {
        let context = ToolContext {
            session_id: "session".into(),
            cwd: String::new(),
            cancel: CancellationToken::new(),
            sink: None,
        };
        for name in ["skills_list", "skill_view", "skill_manage", "learn_skill"] {
            let outcome = dispatch_subagent_tool(
                name,
                &serde_json::json!({}),
                &context,
                &mut Vec::new(),
                &[],
                ExecutionMode::Turbo,
            )
            .await;
            assert!(!outcome.result.success);
            assert!(
                outcome
                    .result
                    .output
                    .contains("only available in the parent run"),
                "{}",
                outcome.result.output
            );
        }
    }

    #[test]
    fn foreground_skill_tools_creation_roots_match_home_policy() {
        let home = dirs::home_dir().unwrap();
        #[cfg(feature = "shared-profile")]
        let base = super::super::storage::config_dir();
        #[cfg(not(feature = "shared-profile"))]
        let base = home.join(".aether-agent");
        for (value, expected) in [
            (None, base.join("skills")),
            (Some("  "), base.join("skills")),
            (Some("relative/skills"), base.join("relative/skills")),
            (Some("~/skills"), home.join("skills")),
            (Some("~\\skills"), home.join("skills")),
            (Some("~"), home.clone()),
        ] {
            assert_eq!(
                foreground_skill_creation_root(&ReviewConfig {
                    create_dir: value.map(str::to_owned),
                    ..Default::default()
                })
                .unwrap(),
                expected
            );
        }
        assert!(foreground_skill_creation_root(&ReviewConfig {
            create_dir: Some("~someone/skills".into()),
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn review_cadence_accumulates_iterations_across_runs_per_session() {
        let runner = AgentRunner::new();
        for _ in 0..6 {
            runner.review_cadence.lock().unwrap().iteration("session");
        }
        assert!(!runner
            .review_cadence
            .lock()
            .unwrap()
            .take_due("session", 10));
        runner.finish("first-run");
        for _ in 0..4 {
            runner.review_cadence.lock().unwrap().iteration("session");
        }
        assert!(!runner.review_cadence.lock().unwrap().take_due("other", 10));
        assert!(runner
            .review_cadence
            .lock()
            .unwrap()
            .take_due("session", 10));
        assert!(!runner
            .review_cadence
            .lock()
            .unwrap()
            .take_due("session", 10));
    }

    #[test]
    fn review_cadence_resets_on_attempted_skill_mutation_and_zero_disables() {
        let mut cadence = ReviewCadence::default();
        for tool in ["learn_skill", "skill_manage"] {
            for _ in 0..10 {
                cadence.iteration("session");
            }
            cadence.tool_attempt("session", tool);
            assert!(!cadence.take_due("session", 10), "{tool}");
        }
        for _ in 0..10 {
            cadence.iteration("session");
        }
        cadence.tool_attempt("session", "read_file");
        assert!(!cadence.take_due("session", 0));
        assert!(cadence.take_due("session", 10));
    }

    #[test]
    fn foreground_run_cancels_review_without_stale_lease_removing_successor() {
        let runner = AgentRunner::new();
        let old = runner.begin_review("review-one", "session", None).unwrap();
        let old_cancel = old.cancel.clone();
        let foreground = runner.begin("foreground", "session").unwrap();
        assert!(old_cancel.is_cancelled());
        assert!(runner.begin_review("manual", "session", None).is_err());
        let successor = runner
            .begin_review("foreground", "session", Some(&foreground))
            .unwrap();
        drop(old);
        assert_eq!(runner.reviews.lock().unwrap()["session"].id, successor.id);
        runner.finish("foreground");
        runner.abort("review-one").unwrap();
        assert!(!successor.cancel.is_cancelled());
        runner.abort("foreground").unwrap();
        assert!(successor.cancel.is_cancelled());
        drop(successor);
        assert!(runner.reviews.lock().unwrap().is_empty());
    }

    #[test]
    fn review_lease_inherits_foreground_cancellation_after_run_finishes() {
        let runner = AgentRunner::new();
        let foreground = runner.begin("run", "session").unwrap();
        let lease = runner
            .begin_review("run", "session", Some(&foreground))
            .unwrap();
        runner.finish("run");
        foreground.cancel();
        assert!(lease.cancel.is_cancelled());
        assert!(runner
            .begin_review("run", "other", Some(&foreground))
            .is_err());
        drop(lease);
        assert!(runner.reviews.lock().unwrap().is_empty());
    }

    #[test]
    fn manual_review_reservation_updates_run_identity_and_obeys_capacity() {
        let runner = AgentRunner::new();
        let lease = runner.begin_review("placeholder", "session", None).unwrap();
        assert!(runner.begin_review("duplicate", "session", None).is_err());
        lease.set_run_id("completed-run").unwrap();
        runner.abort("placeholder").unwrap();
        assert!(!lease.cancel.is_cancelled());
        let others: Vec<_> = (0..3)
            .map(|index| {
                runner
                    .begin_review("other-run", &format!("other-{index}"), None)
                    .unwrap()
            })
            .collect();
        assert!(runner.begin_review("fifth", "fifth-session", None).is_err());
        runner.abort("completed-run").unwrap();
        assert!(lease.cancel.is_cancelled());
        assert!(lease.set_run_id("too-late").is_err());
        drop(lease);
        drop(others);
        assert!(runner.reviews.lock().unwrap().is_empty());
    }

    #[test]
    fn review_provider_retains_parent_unless_auxiliary_is_different() {
        let parent = ProviderConfig {
            id: "parent".into(),
            model: "actual-parent-model".into(),
            ..Default::default()
        };
        let saved_parent = ProviderConfig {
            id: "parent".into(),
            model: "stale-settings-model".into(),
            ..Default::default()
        };
        let auxiliary = ProviderConfig {
            id: "auxiliary".into(),
            model: "review-model".into(),
            ..Default::default()
        };
        let providers = vec![saved_parent, auxiliary];
        let mut config = ReviewConfig::default();
        for id in [None, Some(""), Some("parent")] {
            config.auxiliary_provider_id = id.map(str::to_string);
            let (selected, routed) = review_provider(&parent, &config, &providers).unwrap();
            assert_eq!(selected.model, "actual-parent-model");
            assert!(!routed);
        }
        config.auxiliary_provider_id = Some("auxiliary".into());
        let (selected, routed) = review_provider(&parent, &config, &providers).unwrap();
        assert_eq!(selected.model, "review-model");
        assert!(routed);
        config.auxiliary_provider_id = Some("missing".into());
        assert!(review_provider(&parent, &config, &providers).is_err());
    }

    #[test]
    fn child_artifact_and_terminal_events_share_scoped_identity() {
        let artifact = Artifact {
            stream_revision: None,
            id: "art-1".into(),
            title: "diff".into(),
            artifact_type: "diff".into(),
            language: None,
            content: "change".into(),
            file_path: None,
            timestamp: 1,
        };
        let ToolEvent::Artifact(scoped) =
            scope_subagent_tool_event("child-one", ToolEvent::Artifact(artifact.clone()))
        else {
            panic!("artifact expected")
        };
        let ToolEvent::TerminalOutput { artifact_id, .. } = scope_subagent_tool_event(
            "child-one",
            ToolEvent::TerminalOutput {
                artifact_id: artifact.id.clone(),
                delta: "line".into(),
                stream: "stdout".into(),
            },
        ) else {
            panic!("terminal expected")
        };
        assert_eq!(scoped.id, artifact_id);
        assert_ne!(scoped.id, scope_subagent_artifact("child-two", artifact).id);
    }

    #[test]
    fn request_review_does_not_start_mcp_servers_during_connection() {
        assert!(!mcp_connections_allowed(ExecutionMode::RequestReview));
        assert!(mcp_connections_allowed(ExecutionMode::Turbo));
    }

    #[test]
    fn native_anthropic_helper_snapshot_validates_and_replays_unchanged() {
        let stream = super::super::provider::StreamResult {
            content: "inspect".into(),
            assistant_content: vec![
                serde_json::json!({"type":"thinking","thinking":"private","signature":"opaque-signature"}),
                serde_json::json!({"type":"redacted_thinking","data":"opaque-redacted"}),
                serde_json::json!({"type":"text","text":"inspect"}),
                serde_json::json!({"type":"tool_use","id":"native-t","name":"read_file","input":{"path":"x"}}),
            ],
            tool_calls: vec![super::super::provider::ChatCompletionToolCall {
                id: "native-t".into(),
                call_type: "function".into(),
                function: super::super::provider::ChatCompletionFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"x"}"#.into(),
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let assistant = stream.assistant_message(ProviderKind::Anthropic);
        assert!(assistant.get("tool_calls").is_none());
        assert_eq!(
            assistant["content"],
            serde_json::json!(stream.assistant_content)
        );
        let final_assistant = super::super::provider::StreamResult {
            content: "final".into(),
            assistant_content: vec![serde_json::json!({"type":"text","text":"final"})],
            ..Default::default()
        }
        .assistant_message(ProviderKind::Anthropic);
        let native = vec![
            assistant,
            serde_json::json!({"role":"tool","tool_call_id":"native-t","content":"read"}),
            final_assistant,
        ];
        assert!(valid_provider_messages(&native));
        let persisted = serde_json::to_string(&serde_json::json!({
            "id":"a","role":"assistant","content":"display","timestamp":1,"providerMessages":native
        }))
        .unwrap();
        let replay = replay_fixture(serde_json::from_str(&persisted).unwrap());
        assert_eq!(replay, native);
        assert!(valid_provider_messages(&replay));
        assert!(!valid_provider_messages(&native[..1]));
        assert!(valid_provider_messages(&native[..2]));
    }

    #[test]
    fn native_snapshot_rejects_invalid_tool_blocks_and_reused_ids() {
        let call = serde_json::json!({"type":"tool_use","id":"t","name":"read_file","input":{}});
        let result = serde_json::json!({"role":"tool","tool_call_id":"t","content":"read"});
        for (field, value) in [
            ("id", serde_json::json!(" ")),
            ("id", serde_json::Value::Null),
            ("name", serde_json::json!(" ")),
            ("name", serde_json::Value::Null),
            ("input", serde_json::json!([])),
            ("input", serde_json::Value::Null),
        ] {
            let mut invalid = call.clone();
            invalid[field] = value;
            assert!(
                !valid_provider_messages(&[
                    serde_json::json!({"role":"assistant","content":[invalid]}),
                    result.clone(),
                ]),
                "accepted invalid {field}: {invalid}"
            );
        }
        let assistant = serde_json::json!({"role":"assistant","content":[call]});
        assert!(!valid_provider_messages(&[
            serde_json::json!({"role":"assistant","content":[call,call]}),
            result.clone(),
        ]));
        assert!(!valid_provider_messages(&[
            assistant.clone(),
            result.clone(),
            assistant.clone(),
            result.clone(),
        ]));
        assert!(!valid_provider_messages(&[
            assistant.clone(),
            result.clone(),
            result,
        ]));
        assert!(!valid_provider_messages(&[
            assistant,
            serde_json::json!({"role":"assistant","content":"phantom final before result"}),
        ]));
    }

    #[test]
    fn native_snapshot_optional_mirrors_must_match_without_double_pending() {
        let assistant = serde_json::json!({"role":"assistant","content":[
            {"type":"tool_use","id":"t","name":"read_file","input":{"path":"x","limit":1}},
            {"type":"tool_use","id":"u","name":"read_file","input":{"path":"y"}}
        ]});
        let call = serde_json::json!({"id":"t","type":"function","function":{
            "name":"read_file","arguments":"{ \"limit\": 1, \"path\": \"x\" }"
        }});
        let other = serde_json::json!({"id":"u","type":"function","function":{
            "name":"read_file","arguments":"{\"path\":\"y\"}"
        }});
        let result = serde_json::json!({"role":"tool","tool_call_id":"t","content":"read"});
        let other_result = serde_json::json!({"role":"tool","tool_call_id":"u","content":"read y"});
        for mirrors in [serde_json::Value::Null, serde_json::json!([other, call])] {
            let mut valid = assistant.clone();
            valid["tool_calls"] = mirrors;
            assert!(valid_provider_messages(&[
                valid,
                result.clone(),
                other_result.clone()
            ]));
        }
        for (field, value) in [
            ("id", serde_json::json!("phantom")),
            ("id", serde_json::json!(" ")),
            (
                "function",
                serde_json::json!({"name":"write_file","arguments":"{\"path\":\"x\",\"limit\":1}"}),
            ),
            (
                "function",
                serde_json::json!({"name":"read_file","arguments":"{\"path\":\"wrong\",\"limit\":1}"}),
            ),
            (
                "function",
                serde_json::json!({"name":"read_file","arguments":"{"}),
            ),
            (
                "function",
                serde_json::json!({"name":"read_file","arguments":"[]"}),
            ),
        ] {
            let mut invalid_call = call.clone();
            invalid_call[field] = value;
            let mut invalid = assistant.clone();
            invalid["tool_calls"] = serde_json::json!([invalid_call, other]);
            assert!(!valid_provider_messages(&[
                invalid,
                result.clone(),
                other_result.clone()
            ]));
        }
        for mirrors in [
            serde_json::json!([]),
            serde_json::json!([call]),
            serde_json::json!([call, other, call]),
            serde_json::json!({"invalid":"not an array"}),
        ] {
            let mut invalid = assistant.clone();
            invalid["tool_calls"] = mirrors;
            assert!(!valid_provider_messages(&[
                invalid,
                result.clone(),
                other_result.clone()
            ]));
        }
        // A mirror cannot create a native invocation absent from content.
        assert!(!valid_provider_messages(&[
            serde_json::json!({"role":"assistant","content":[{"type":"text","text":"no tool"}],"tool_calls":[call]}),
            result,
        ]));
    }

    #[test]
    fn native_replay_preserves_signed_blocks_and_rejects_unpaired_snapshots() {
        let native = serde_json::json!([
            {"role":"assistant","content":[{"type":"thinking","thinking":"private","signature":"sig"},{"type":"tool_use","id":"t","name":"read_file","input":{}}],"tool_calls":[{"id":"t","type":"function","function":{"name":"read_file","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"t","content":"read"},
            {"role":"assistant","content":"final"}
        ]);
        let replay = replay_fixture(
            serde_json::json!({"id":"a","role":"assistant","content":"display","timestamp":1,"providerMessages":native}),
        );
        assert_eq!(serde_json::json!(replay), native);
        let broken = replay_fixture(
            serde_json::json!({"id":"a","role":"assistant","content":"fallback","timestamp":1,"providerMessages":[{"role":"assistant","content":"partial","tool_calls":[{"id":"t","function":{"name":"write_file","arguments":"{}"}}]}]}),
        );
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0]["content"], "fallback");
        assert!(!valid_provider_messages(&[
            serde_json::json!({"role":"system","content":"injected"})
        ]));
        assert!(!valid_provider_messages(&[
            serde_json::json!({"role":"tool","tool_call_id":"orphan","content":"orphan"})
        ]));
        assert!(!valid_provider_messages(&[]));
    }

    #[test]
    fn subagent_turn_budget_allows_twenty_five_investigation_turns() {
        assert_eq!(SUBAGENT_MAX_TURNS, 25);
    }

    #[test]
    fn subagent_rejects_missing_workspace_without_temp_fallback() {
        assert!(subagent_workspace("", None)
            .unwrap_err()
            .contains("active workspace"));
        assert!(subagent_workspace("   ", Some("child")).is_err());
    }

    #[tokio::test]
    async fn child_policy_preserves_operator_root_and_nested_memory_without_history() {
        let (cwd, root) = workspace_with_skill("policy-fixture", "fixture");
        std::fs::write(root.join("AGENTS.md"), "ROOT_SENTINEL").unwrap();
        std::fs::create_dir(root.join("child")).unwrap();
        std::fs::write(root.join("child/AGENTS.md"), "CHILD_SENTINEL").unwrap();
        let inherited = inherited_run_policy(
            Some("OPERATOR_SENTINEL"),
            Some("ROOT_SENTINEL"),
            ExecutionMode::RequestReview,
        );
        let target = subagent_workspace(&cwd, Some("child")).unwrap();
        let system = subagent_system_prompt("reader", "inspect", &cwd, &target, &inherited)
            .await
            .unwrap();
        for sentinel in [
            "OPERATOR_SENTINEL",
            "ROOT_SENTINEL",
            "CHILD_SENTINEL",
            "REQUEST REVIEW",
            LEAN_WORKFLOW,
        ] {
            assert!(system.contains(sentinel), "missing {sentinel}: {system}");
        }
        assert_eq!(system.matches("ROOT_SENTINEL").count(), 1);
        assert!(!system.contains("PRIVATE_PARENT_HISTORY"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn child_options_preserve_every_parent_completion_setting() {
        let parent = CompletionOptions {
            thinking: true,
            reasoning_effort: ReasoningEffort::High,
            temperature: 0.91,
            max_tokens: Some(1234),
            stream: false,
        };
        let child = subagent_completion_options(&parent);
        assert_eq!(child.thinking, parent.thinking);
        assert_eq!(child.reasoning_effort, parent.reasoning_effort);
        assert_eq!(child.temperature, parent.temperature);
        assert_eq!(child.max_tokens, parent.max_tokens);
        assert_eq!(child.stream, parent.stream);
    }

    #[tokio::test]
    async fn request_review_blocks_child_write_and_exec_before_side_effects() {
        let (cwd, root) = workspace_with_skill("deny-fixture", "fixture");
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd,
            cancel: CancellationToken::new(),
            sink: None,
        };
        let mut skills = Vec::new();
        for name in [
            "write_file",
            "edit_file",
            "run_command",
            "learn_skill",
            "manage_task",
            "mcp_remote",
            "unknown",
        ] {
            let args = serde_json::json!({"path":"forbidden.txt", "content":"written", "command":"exit 99", "action":"kill", "taskId":"none"});
            let denied = dispatch_subagent_tool(
                name,
                &args,
                &context,
                &mut skills,
                &[],
                ExecutionMode::RequestReview,
            )
            .await;
            assert!(!denied.result.success, "{name}");
            assert!(
                denied.result.output.contains("Request Review"),
                "{}",
                denied.result.output
            );
        }
        assert!(!root.join("forbidden.txt").exists());
        assert!(tool_policy_error(
            ExecutionMode::RequestReview,
            "mcp_read",
            &serde_json::json!({}),
            true
        )
        .is_some());
        assert!(tool_policy_error(
            ExecutionMode::RequestReview,
            "read_file",
            &serde_json::json!({}),
            false
        )
        .is_none());
        assert!(tool_policy_error(
            ExecutionMode::RequestReview,
            "spawn_subagent",
            &serde_json::json!({}),
            false
        )
        .is_none());
        assert!(tool_policy_error(
            ExecutionMode::Turbo,
            "write_file",
            &serde_json::json!({}),
            false
        )
        .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn synthesis_cancel_error_empty_and_budget_never_report_success() {
        use super::super::provider::StreamResult;
        let token = CancellationToken::new();
        let cancelled = StreamResult {
            cancelled: true,
            ..Default::default()
        };
        assert!(
            subagent_synthesis_failure(Ok(cancelled), "partial", &token, true)
                .unwrap()
                .cancelled
        );
        let failed =
            subagent_synthesis_failure(Err("HTTP 503".into()), "partial", &token, true).unwrap();
        assert!(!failed.success && !failed.cancelled);
        assert!(failed.output.contains("503") && failed.output.contains("partial"));
        assert!(
            subagent_synthesis_failure(Ok(StreamResult::default()), "", &token, true).is_some()
        );
        assert!(
            subagent_synthesis_failure(Ok(StreamResult::default()), "report", &token, false)
                .is_some()
        );
        assert!(
            subagent_synthesis_failure(Ok(StreamResult::default()), "report", &token, true)
                .is_none()
        );
        token.cancel();
        assert!(
            subagent_synthesis_failure(Ok(StreamResult::default()), "report", &token, true)
                .unwrap()
                .cancelled
        );
    }

    fn replay_fixture(value: serde_json::Value) -> Vec<serde_json::Value> {
        let message: ChatMessage = serde_json::from_value(value).unwrap();
        let mut conversation = Vec::new();
        append_history_message(&mut conversation, &message);
        conversation
    }

    #[test]
    fn image_token_estimate_counts_images_not_base64_length() {
        let mut message: ChatMessage = serde_json::from_value(serde_json::json!({
            "id":"image", "role":"user", "content":"describe", "timestamp":1,
            "imageInputs":[{"sourcePath":"missing.png", "mediaType":"image/png", "data":"iVBORw0KGgo="}]
        })).unwrap();
        assert_eq!(
            estimate_messages_tokens(&[message.clone()], "text"),
            3 + 1600
        );
        message.image_inputs.as_mut().unwrap()[0].data = "A".repeat(4_000_000);
        assert_eq!(
            estimate_messages_tokens(&[message.clone()], "text"),
            3 + 1600
        );
        let image = message.image_inputs.as_ref().unwrap()[0].clone();
        message.image_inputs.as_mut().unwrap().push(image);
        assert_eq!(
            estimate_messages_tokens(&[message.clone()], "text"),
            3 + 3200
        );
        message.image_inputs = None;
        assert_eq!(estimate_messages_tokens(&[message], "text"), 3);
    }

    #[test]
    fn image_replay_preserves_persisted_content_without_reading_paths() {
        let value = serde_json::json!({
            "id":"image", "role":"user", "content":"describe this", "timestamp":1,
            "attachments":[{"id":"file", "name":"gone.png", "path":"Z:/missing/gone.png", "isDir":false}],
            "imageInputs":[{"sourcePath":"Z:/missing/gone.png", "mediaType":"image/png", "data":"iVBORw0KGgo="}],
            "providerMessages":[{"role":"assistant","content":"must not replace user text"}]
        });
        let persisted = serde_json::to_string(&value).unwrap();
        let restored: ChatMessage = serde_json::from_str(&persisted).unwrap();
        super::super::images::validate_request_images(
            std::slice::from_ref(&restored),
            &ProviderConfig::default(),
        )
        .unwrap();
        let mut replay = Vec::new();
        append_history_message(&mut replay, &restored);
        assert_eq!(
            replay,
            vec![serde_json::json!({"role":"user","content":[
                {"type":"text","text":"describe this"},
                {"type":"image_url","image_url":{"url":"data:image/png;base64,iVBORw0KGgo="}}
            ]})]
        );
        let mut legacy = value;
        legacy.as_object_mut().unwrap().remove("imageInputs");
        assert_eq!(
            replay_fixture(legacy),
            vec![serde_json::json!({"role":"user","content":"describe this"})]
        );
        let assistant = serde_json::json!({"id":"a","role":"assistant","content":"display","timestamp":1,
            "providerMessages":[{"role":"assistant","content":"opaque provider transcript"}]});
        assert_eq!(
            replay_fixture(assistant),
            vec![serde_json::json!({"role":"assistant","content":"opaque provider transcript"})]
        );
    }

    #[test]
    fn replay_segments_preserves_two_tool_turns_without_duplicate_aggregate_text() {
        let call = |id: &str, output: &str| {
            serde_json::json!({
                "id":id,"name":"read_file","args":{"path":id},"status":"done","result":output
            })
        };
        let replay = replay_fixture(serde_json::json!({
            "id":"a","role":"assistant","timestamp":1,"preamble":"inspect first",
            "content":"inspect secondfinal answer", "toolCalls":[call("t1","one"),call("t2","two")],
            "segments":[
                {"id":"s1","text":"inspect first","toolCalls":[call("t1","one")]},
                {"id":"s2","text":"inspect second","toolCalls":[call("t2","two")]},
                {"id":"s3","text":"final answer"}
            ]
        }));
        assert_eq!(replay.len(), 5);
        assert_eq!(replay[0]["content"], "inspect first");
        assert_eq!(replay[1]["tool_call_id"], "t1");
        assert_eq!(replay[2]["content"], "inspect second");
        assert_eq!(replay[3]["tool_call_id"], "t2");
        assert_eq!(replay[4]["content"], "final answer");
    }

    #[test]
    fn replay_filters_phantom_invalid_and_duplicate_calls_but_keeps_terminal_pairs() {
        let replay = replay_fixture(serde_json::json!({
            "id":"a","role":"assistant","content":"partial","timestamp":1,
            "toolCalls":[
                {"id":"pending","name":"write_file","args":{},"status":"pending"},
                {"id":"running","name":"write_file","args":{},"status":"running","result":"stale"},
                {"id":"bad","name":"write_file","args":{},"argumentsRaw":"{","status":"error","result":"bad json"},
                {"id":"ok","name":"read_file","args":{},"status":"done","result":"read"},
                {"id":"ok","name":"read_file","args":{},"status":"done","result":"duplicate"},
                {"id":"cancel","name":"run_command","args":{},"status":"cancelled","result":"Cancelled by operator"}
            ]
        }));
        let calls = replay[0]["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(replay.len(), 3);
        assert_eq!(replay[1]["tool_call_id"], "ok");
        assert_eq!(replay[2]["tool_call_id"], "cancel");
    }

    #[test]
    fn replay_legacy_preamble_precedes_tools_and_final_content_follows_results() {
        let replay = replay_fixture(serde_json::json!({
            "id":"a","role":"assistant","content":"final","preamble":"opening","timestamp":1,
            "segments":[],
            "toolCalls":[{"id":"t","name":"read_file","args":"{\"path\":\"x\"}","status":"done","result":"read"}]
        }));
        assert_eq!(replay.len(), 3);
        assert_eq!(replay[0]["content"], "opening");
        assert_eq!(replay[1]["role"], "tool");
        assert_eq!(replay[2]["content"], "final");
    }

    #[test]
    fn manual_review_replays_all_completed_conversation() {
        let messages: Vec<ChatMessage> = serde_json::from_value(serde_json::json!([
            {"id":"u1", "role":"user", "content":"Old", "timestamp":1},
            {"id":"a1", "role":"assistant", "content":"Old answer", "timestamp":2},
            {"id":"u2", "role":"user", "content":"Fix parser", "timestamp":3},
            {"id":"a2", "role":"assistant", "content":"Fixed", "timestamp":4,"runId":"r2","runStatus":"completed",
             "toolCalls":[{"id":"t1","name":"run_command","args":{},"status":"done","result":"tests pass"}]}
        ])).unwrap();
        let (run, snapshot) = completed_review_snapshot(&messages).unwrap();
        assert_eq!(run, "r2");
        assert_eq!(snapshot.len(), 5);
        assert_eq!(snapshot[0]["content"], "Old");
        assert_eq!(snapshot[1]["content"], "Old answer");
        assert_eq!(snapshot[2]["content"], "Fix parser");
        assert!(serde_json::to_string(&snapshot)
            .unwrap()
            .contains("tests pass"));
        let mut failed = messages.clone();
        failed.last_mut().unwrap().run_status = Some("failed".into());
        assert!(completed_review_snapshot(&failed).is_err());
        let mut pending = messages;
        pending.push(
            serde_json::from_value(
                serde_json::json!({"id":"u3","role":"user","content":"Next","timestamp":5}),
            )
            .unwrap(),
        );
        assert!(completed_review_snapshot(&pending).is_err());
    }

    #[test]
    fn manual_review_rejects_identified_runs_without_completed_status() {
        let mut messages: Vec<ChatMessage> = serde_json::from_value(serde_json::json!([
            {"id":"u1", "role":"user", "content":"Task", "timestamp":1},
            {"id":"a1", "role":"assistant", "content":"Partial answer", "timestamp":2,"runId":"r1",
             "toolCalls":[{"id":"t1","name":"run_command","args":{},"status":"running"}]}
        ]))
        .unwrap();
        assert!(completed_review_snapshot(&messages).is_err());
        messages[1].run_id = None;
        assert!(completed_review_snapshot(&messages).is_err());
        messages[1].tool_calls.as_mut().unwrap()[0].status = "done".into();
        assert!(completed_review_snapshot(&messages).is_ok());
    }

    #[test]
    fn provider_keys_are_redacted_from_errors() {
        let provider = ProviderConfig {
            api_key: "secret".into(),
            ..Default::default()
        };
        assert_eq!(
            redact_provider_error("failed with secret".into(), &provider),
            "failed with [redacted]"
        );
    }

    #[test]
    fn provider_errors_redact_credentials_from_extra_headers() {
        let provider = ProviderConfig {
            api_key: "sk-live".into(),
            extra_headers: [
                ("Authorization", "Bearer tok-123"),
                ("X-Api-Key", "key-456"),
                ("X-Trace-Id", "trace-keep-me"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
            ..Default::default()
        };
        let redacted = redact_provider_error(
            "upstream rejected Bearer tok-123 using sk-live and key-456 (trace-keep-me)".into(),
            &provider,
        );
        assert!(!redacted.contains("tok-123"), "{redacted}");
        assert!(!redacted.contains("key-456"), "{redacted}");
        assert!(!redacted.contains("sk-live"), "{redacted}");
        assert!(redacted.contains("trace-keep-me"), "{redacted}");
    }

    #[test]
    fn micro_compaction_preserves_recent_turns_and_prunes_older_verbose_tools() {
        let long_output = "line\n".repeat(60) + &"a".repeat(2000);
        let mut messages: Vec<ChatMessage> = serde_json::from_value(serde_json::json!([
            {"id":"u1", "role":"user", "content":"Turn 1 user request", "timestamp":1},
            {"id":"a1", "role":"assistant", "content":"Turn 1 answer", "timestamp":2,
             "toolCalls":[{"id":"t1","name":"read_file","args":{"path":"huge.txt"},"status":"done","result":long_output}]},
            {"id":"u2", "role":"user", "content":"Turn 2 user request", "timestamp":3},
            {"id":"a2", "role":"assistant", "content":"Turn 2 answer", "timestamp":4,
             "toolCalls":[{"id":"t2","name":"read_file","args":{"path":"recent.txt"},"status":"done","result":long_output}]},
            {"id":"u3", "role":"user", "content":"Turn 3 user request", "timestamp":5},
            {"id":"a3", "role":"assistant", "content":"Turn 3 active response", "timestamp":6}
        ])).unwrap();

        messages[1].segments = Some(vec![super::super::types::MessageSegment {
            id: "canonical".into(),
            text: Some("Turn 1 answer".into()),
            tool_calls: messages[1].tool_calls.clone(),
        }]);
        messages[1].provider_messages = Some(vec![
            serde_json::json!({"role":"tool","tool_call_id":"native-call","content":long_output}),
        ]);
        let event = apply_micro_compaction(&mut messages, "r1", "s1", 50);
        assert!(
            messages[1].provider_messages.as_ref().unwrap()[0]["content"]
                .as_str()
                .unwrap()
                .starts_with("[Output pruned:")
        );
        assert!(messages[1].segments.as_ref().unwrap()[0]
            .tool_calls
            .as_ref()
            .unwrap()[0]
            .result
            .as_ref()
            .unwrap()
            .starts_with("[Output pruned:"));
        assert!(event.is_some());
        let evt = event.unwrap();
        assert_eq!(evt.pruned_tool_count, 1);
        assert!(evt.estimated_tokens_saved > 0);

        let t1_res = messages[1].tool_calls.as_ref().unwrap()[0]
            .result
            .as_ref()
            .unwrap();
        assert!(t1_res.starts_with("[Output pruned:"));
        assert!(t1_res.contains("from 'read_file'"));

        let t2_res = messages[3].tool_calls.as_ref().unwrap()[0]
            .result
            .as_ref()
            .unwrap();
        assert_eq!(t2_res, &long_output);
    }

    #[test]
    fn micro_compaction_noop_under_threshold() {
        let mut messages: Vec<ChatMessage> = serde_json::from_value(serde_json::json!([
            {"id":"u1", "role":"user", "content":"Hi", "timestamp":1},
            {"id":"a1", "role":"assistant", "content":"Hello", "timestamp":2},
            {"id":"u2", "role":"user", "content":"How are you?", "timestamp":3},
            {"id":"a2", "role":"assistant", "content":"Good!", "timestamp":4}
        ]))
        .unwrap();

        let event = apply_micro_compaction(&mut messages, "r1", "s1", 100_000);
        assert!(event.is_none());
    }

    fn workspace_with_skill(skill_name: &str, body: &str) -> (String, std::path::PathBuf) {
        let root = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("aether-runner-{}", Uuid::new_v4()));
        let skill_dir = root.join(".aether").join("skills").join(skill_name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {skill_name}\ndescription: Regression fixture.\n---\n{body}\n"),
        )
        .unwrap();
        (root.to_string_lossy().to_string(), root)
    }

    /// Regression guard for the subagent tool dispatcher: a subagent that calls
    /// `use_skill` must activate the catalogued skill and receive its SKILL.md
    /// body, exactly like the parent run — never "Unknown tool".
    #[tokio::test]
    async fn subagent_use_skill_tool_call_activates_the_catalogued_skill() {
        let (cwd, root) = workspace_with_skill("subagent-skill-fixture", "Step one. Step two.");
        let skill_paths = workspace_skill_paths(&cwd, &[]);
        let mut skills = load_skills(&skill_paths).await;
        assert!(
            skills
                .iter()
                .any(|skill| skill.name == "subagent-skill-fixture"),
            "fixture skill must be discoverable from the workspace catalog"
        );
        let context = ToolContext {
            session_id: "test-session".into(),
            cwd: cwd.clone(),
            cancel: CancellationToken::new(),
            sink: None,
        };

        let outcome = dispatch_subagent_tool(
            "use_skill",
            &serde_json::json!({"name": "subagent-skill-fixture"}),
            &context,
            &mut skills,
            &skill_paths,
            ExecutionMode::Turbo,
        )
        .await;

        assert!(
            outcome.result.success,
            "use_skill inside a subagent must succeed, got: {}",
            outcome.result.output
        );
        assert!(
            !outcome.result.output.contains("Unknown tool"),
            "use_skill must not fall through to the native dispatcher: {}",
            outcome.result.output
        );
        assert!(
            outcome.result.output.contains("Step one. Step two."),
            "subagent must receive the validated SKILL.md body: {}",
            outcome.result.output
        );
        assert_eq!(
            outcome.activated_skill.map(|skill| skill.name),
            Some("subagent-skill-fixture".to_string()),
            "the activated skill must be reported so the UI event can be emitted"
        );

        // An unknown skill must fail loudly instead of silently succeeding.
        let missing = dispatch_subagent_tool(
            "use_skill",
            &serde_json::json!({"name": "no-such-skill"}),
            &context,
            &mut skills,
            &skill_paths,
            ExecutionMode::Turbo,
        )
        .await;
        assert!(!missing.result.success);
        assert!(missing
            .result
            .output
            .contains("was not found in the catalog"));
        assert!(missing.activated_skill.is_none());

        // Native tools must still dispatch untouched.
        let native = dispatch_subagent_tool(
            "list_directory",
            &serde_json::json!({"path": "."}),
            &context,
            &mut skills,
            &skill_paths,
            ExecutionMode::Turbo,
        )
        .await;
        assert!(native.result.success, "{}", native.result.output);
        assert!(native.activated_skill.is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn use_skill_hot_reloads_dynamically_when_skill_added_to_disk() {
        let root = std::env::temp_dir().join(format!("aether-hotreload-{}", Uuid::new_v4()));
        let skill_paths = vec![root.to_string_lossy().to_string()];
        let mut skills = Vec::new(); // Starts completely empty

        // 1. Initial attempt fails because skill does not exist on disk
        let res1 = activate_skill(&mut skills, "dynamic-skill", &skill_paths).await;
        assert!(!res1.result.success);

        // 2. Skill is added to disk while session is ongoing
        let skill_dir = root.join("dynamic-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: dynamic-skill\ndescription: Added live mid-session\n---\n# Live Skill\nUse `run_command` and `read_file`.\n",
        )
        .unwrap();

        // 3. Next use_skill call immediately hot-reloads and succeeds without restart
        let res2 = activate_skill(&mut skills, "dynamic-skill", &skill_paths).await;
        assert!(res2.result.success, "{}", res2.result.output);
        assert!(res2.result.output.contains("Live Skill"));
        assert!(skills.iter().any(|s| s.name == "dynamic-skill"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn subagent_finding_detection_and_safe_prefix_regression() {
        // Safe prefix handles multibyte boundaries cleanly
        let multibyte = "🚀 AetherAgent autonomous subagent findings with UTF-8: 日本語";
        assert_eq!(safe_prefix(multibyte, 5), "🚀 Aet");
        assert_eq!(safe_prefix("short", 10), "short");

        // Short procedural boilerplate is rejected
        assert!(!is_substantive_finding(""));
        assert!(!is_substantive_finding("   "));
        assert!(!is_substantive_finding(
            "Let me run grep to search for logs."
        ));
        assert!(!is_substantive_finding("I will check the directory."));
        assert!(!is_substantive_finding("Running Get-ChildItem..."));

        // Substantive findings (steps, tables, conclusions, or long text) are accepted
        assert!(is_substantive_finding(
            "Step 1: tested use_skill with 3 skills. All passed."
        ));
        assert!(is_substantive_finding(
            "| File | Bytes |\n| --- | --- |\n| a.log | 120 |"
        ));
        assert!(is_substantive_finding(
            "Found 10 failure lines in red.log with code 500 across 3 sessions."
        ));
    }

    #[derive(Clone, Default)]
    struct ToolEventCollector(Arc<Mutex<Vec<serde_json::Value>>>);

    impl super::super::events::EventSink for ToolEventCollector {
        fn emit(&self, topic: &str, payload: serde_json::Value) -> Result<(), String> {
            assert_eq!(topic, "agent-event");
            self.0.lock().unwrap().push(payload);
            Ok(())
        }
    }

    #[tokio::test]
    async fn runner_retry_partial_tools_survive_durable_checkpoint_and_final_save() {
        use super::super::{session_journal, storage, types::CheckpointDelta, types::SessionData};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        struct RetrySink(tokio::sync::mpsc::UnboundedSender<serde_json::Value>);
        impl super::super::events::EventSink for RetrySink {
            fn emit(&self, topic: &str, payload: serde_json::Value) -> Result<(), String> {
                assert_eq!(topic, "agent-event");
                self.0.send(payload).map_err(|error| error.to_string())
            }
        }

        async fn accept_request(
            listener: &tokio::net::TcpListener,
        ) -> (tokio::net::TcpStream, serde_json::Value) {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            let header_end = loop {
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
                if let Some(offset) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break offset + 4;
                }
            };
            let length = String::from_utf8_lossy(&request[..header_end])
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            while request.len() < header_end + length {
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let body = serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            (socket, body)
        }

        fn apply_event(message: &mut ChatMessage, event: &serde_json::Value) {
            use super::super::types::MessageSegment;
            let data = &event["data"];
            match event["type"].as_str().unwrap() {
                "tool_update" => {
                    let incoming: ToolCallItem = serde_json::from_value(data["tool"].clone()).unwrap();
                    let segments = message.segments.get_or_insert_with(|| {
                        vec![MessageSegment {
                            id: "seg_0".into(),
                            text: Some(String::new()),
                            tool_calls: Some(vec![]),
                        }]
                    });
                    // Samakan penggantian berdasarkan ID pada agregat dan segmen frontend.
                    for tools in [
                        message.tool_calls.get_or_insert_with(Vec::new),
                        segments
                            .last_mut()
                            .unwrap()
                            .tool_calls
                            .get_or_insert_with(Vec::new),
                    ] {
                        if let Some(old) = tools.iter_mut().find(|tool| tool.id == incoming.id) {
                            *old = incoming.clone();
                        } else {
                            tools.push(incoming.clone());
                        }
                    }
                }
                "notice" if data["resetPartial"] == true => {
                    message.checkpoint_epoch = Some(message.checkpoint_epoch.unwrap_or(0) + 1);
                    message.reasoning = Some(String::new());
                }
                "chunk" => {
                    let text = data["text"].as_str().unwrap();
                    message.content.push_str(text);
                    let segments = message.segments.as_mut().unwrap();
                    segments.push(MessageSegment {
                        id: format!("seg_{}", segments.len()),
                        text: Some(text.into()),
                        tool_calls: Some(vec![]),
                    });
                }
                "provider_state" => {
                    message.provider_messages =
                        Some(serde_json::from_value(data["providerMessages"].clone()).unwrap());
                }
                "done" => {
                    assert_eq!(data["cancelled"], false);
                    assert_eq!(data["successful"], true);
                    message.is_streaming = Some(false);
                    message.run_status = Some("completed".into());
                }
                "error" => panic!("unexpected runner error: {data}"),
                _ => {}
            }
        }

        struct RetryDirectory(std::path::PathBuf);
        impl Drop for RetryDirectory {
            fn drop(&mut self) {
                // Bersihkan hanya direktori fixture milik pengujian ini, termasuk saat assertion gagal.
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let session_id = format!("retry-checkpoint-{}", Uuid::new_v4());
        let root = std::env::temp_dir().join(&session_id);
        std::fs::create_dir_all(&root).unwrap();
        let _directory = RetryDirectory(root.clone());
        std::fs::write(root.join("answer.txt"), "successful retry tool output").unwrap();
        storage::register_session_test_directory(&session_id, root.clone());
        let initial: SessionData = serde_json::from_value(serde_json::json!({
                "session": {"id":session_id,"title":"Retry checkpoint","createdAt":1,"updatedAt":1,"messageCount":2},
                "messages": [
                    {"id":"user","role":"user","content":"Read answer.txt","timestamp":1},
                    {"id":"assistant","role":"assistant","content":"","timestamp":2,"runId":"retry-run","isStreaming":true}
                ], "artifacts":[]
            })).unwrap();
        storage::save_session(initial).await.unwrap();
        let mut page = storage::load_session_page(&session_id, None, 2, None)
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (checkpointed, checkpoint_gate) = tokio::sync::oneshot::channel();
        let (retry_checkpointed, retry_gate) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = accept_request(&listener).await;
            for event in [
                serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":7,"id":"provider-attempt-a","function":{"name":"write_file","arguments":"{\"path\":\"abandoned-"}}]}}]}),
                serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":7,"function":{"arguments":"a.txt\""}},{"index":11,"id":"provider-abandoned-placeholder","function":{"arguments":"{"}}]}}]}),
            ] {
                socket
                    .write_all(format!("data: {event}\n\n").as_bytes())
                    .await
                    .unwrap();
            }
            socket.flush().await.unwrap();
            // Jangan putuskan upaya A sebelum checkpoint parsial benar-benar tersimpan.
            checkpoint_gate.await.unwrap();
            socket.shutdown().await.unwrap();
            drop(socket);
            let (mut socket, _) = accept_request(&listener).await;
            retry_gate.await.unwrap();
            let event = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":7,"id":"provider-attempt-b","function":{"name":"read_file","arguments":"{\"path\":\"answer.txt\"}"}}]}}]});
            socket
                .write_all(format!("data: {event}\n\ndata: [DONE]\n\n").as_bytes())
                .await
                .unwrap();
            socket.shutdown().await.unwrap();
            drop(socket);
            let (mut socket, body) = accept_request(&listener).await;
            socket.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"Retry finished\"}}]}\n\ndata: [DONE]\n\n").await.unwrap();
            socket.shutdown().await.unwrap();
            body
        });
        let request: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "runId":"retry-run", "sessionId":session_id,
            "provider":{"baseUrl":format!("http://{address}"),"model":"retry-fixture"},
            "messages":[page.data.messages[0].clone()], "maxTurns":2,
            "autoCompactionEnabled":false, "reviewConfig":{"createDir":root.join("skills")}
        }))
        .unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let cwd = root.to_string_lossy().into_owned();
        let runner = tokio::spawn(async move {
            AgentRunner::new()
                .run(
                    AgentHost::new(RetrySink(sender)),
                    cwd,
                    request,
                    CancellationToken::new(),
                )
                .await;
        });
        let mut checkpointed = Some(checkpointed);
        let mut retry_checkpointed = Some(retry_checkpointed);
        let mut events = Vec::new();
        let mut pending_a = None;
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(event) = receiver.recv().await {
                apply_event(&mut page.data.messages[1], &event);
                if event["type"] == "tool_update"
                    && event["data"]["tool"]["argumentsRaw"] == "{"
                    && checkpointed.is_some()
                {
                    pending_a = page.data.messages[1].tool_calls.clone();
                    storage::checkpoint_session_delta(
                        &session_id,
                        CheckpointDelta {
                            run_id: "retry-run".into(),
                            base_revision: page.window.revision.clone(),
                            sequence: 1,
                            messages: vec![page.data.messages[1].clone()],
                            artifacts: vec![],
                        },
                    )
                    .await
                    .unwrap();
                    checkpointed.take().unwrap().send(()).unwrap();
                }
                if event["type"] == "notice" && event["data"]["attempt"] == 2 {
                    storage::checkpoint_session_delta(
                        &session_id,
                        CheckpointDelta {
                            run_id: "retry-run".into(),
                            base_revision: page.window.revision.clone(),
                            sequence: 2,
                            messages: vec![page.data.messages[1].clone()],
                            artifacts: vec![],
                        },
                    )
                    .await
                    .unwrap();
                    retry_checkpointed.take().unwrap().send(()).unwrap();
                }
                let done = event["type"] == "done";
                events.push(event);
                if done {
                    break;
                }
            }
            runner.await.unwrap();
        })
        .await
        .expect("the retry fixture must finish without a provider stall");
        let provider_request = server.await.unwrap();
        let path = root.join(format!("{session_id}.json"));
        assert!(session_journal::path_for(&path).exists());
        let final_save = storage::save_session_page(page.data, page.window).await;
        eprintln!("retry final save: {:?}", final_save.as_ref().map(|_| ()));
        final_save.expect("retry arguments must not overwrite durable attempt A progress");
        let stored = storage::load_session(&session_id).await.unwrap();
        let message = &stored.messages[1];
        let tools = message.tool_calls.as_ref().unwrap();
        let segment_tools = message.segments.as_ref().unwrap()[0]
            .tool_calls
            .as_ref()
            .unwrap();
        assert_eq!(
            serde_json::to_value(segment_tools).unwrap(),
            serde_json::to_value(tools).unwrap()
        );
        let pending_a = pending_a.unwrap();
        assert_eq!(pending_a.len(), 2);
        assert_eq!(
            pending_a[0].arguments_raw.as_deref(),
            Some("{\"path\":\"abandoned-a.txt\"")
        );
        assert_eq!(
            tools.len(),
            3,
            "both abandoned placeholders must survive beside the successful call"
        );
        let retry_notice = events
            .iter()
            .position(|event| event["type"] == "notice" && event["data"]["attempt"] == 2)
            .unwrap();
        assert_eq!(
            events[retry_notice]["data"]["resetPartial"], true,
            "tool-only deltas are attempt progress"
        );
        assert_eq!(message.checkpoint_epoch, Some(1));
        for pending in pending_a {
            let terminal = tools.iter().find(|tool| tool.id == pending.id).unwrap();
            assert_eq!(terminal.status, "cancelled");
            assert_eq!(terminal.name, pending.name);
            assert_eq!(terminal.args, pending.args);
            assert_eq!(terminal.arguments_raw, pending.arguments_raw);
            assert_eq!(terminal.started_at, pending.started_at);
            assert!(terminal
                .result
                .as_ref()
                .is_some_and(|result| !result.is_empty()));
            assert!(terminal.completed_at.unwrap() >= terminal.started_at.unwrap());
            let cancelled = events
                .iter()
                .position(|event| {
                    event["type"] == "tool_update"
                        && event["data"]["tool"]["id"] == pending.id
                        && event["data"]["tool"]["status"] == "cancelled"
                })
                .unwrap();
            assert!(
                cancelled < retry_notice,
                "close pending calls before announcing the next attempt"
            );
            assert_eq!(
                serde_json::to_value(terminal).unwrap(),
                events[cancelled]["data"]["tool"],
                "abandoned terminal data is immutable through final save"
            );
        }
        let successful = tools.iter().find(|tool| tool.name == "read_file").unwrap();
        assert_eq!(successful.status, "done");
        assert_eq!(successful.args, serde_json::json!({"path":"answer.txt"}));
        assert!(successful
            .result
            .as_ref()
            .unwrap()
            .contains("successful retry tool output"));
        let updates: Vec<_> = events
            .iter()
            .filter(|event| {
                event["type"] == "tool_update" && event["data"]["tool"]["id"] == successful.id
            })
            .map(|event| event["data"]["tool"]["status"].as_str().unwrap())
            .collect();
        assert_eq!(
            updates,
            ["pending", "running", "done"],
            "streaming and preparation must share the successful attempt UI ID"
        );
        assert_eq!(
            provider_request["messages"][2]["tool_calls"][0]["id"],
            "provider-attempt-b"
        );
        assert_eq!(
            provider_request["messages"][3]["tool_call_id"],
            "provider-attempt-b"
        );
        assert!(!provider_request.to_string().contains("provider-attempt-a"));
        let transcript = message.provider_messages.as_ref().unwrap();
        assert_eq!(transcript[0]["tool_calls"][0]["id"], "provider-attempt-b");
        assert_eq!(transcript[1]["tool_call_id"], "provider-attempt-b");
        assert_eq!(message.run_status.as_deref(), Some("completed"));
        assert!(!session_journal::path_for(&path).exists());
        assert!(!session_journal::is_active(&path));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runner_terminal_cancel_preserves_malformed_error_and_checkpoint_finalization() {
        use super::super::{session_journal, storage, types::CheckpointDelta, types::SessionData};

        let events = ToolEventCollector::default();
        let session_id = format!("terminal-cancel-{}", Uuid::new_v4());
        let root = std::env::temp_dir().join(&session_id);
        storage::register_session_test_directory(&session_id, root.clone());
        let emitter = EmitterContext {
            app: AgentHost::new(events.clone()),
            run_id: "malformed-run".into(),
            session_id: session_id.clone(),
        };
        let arguments_raw = r#"{"command":"#;
        let error = format!(
            "Tool 'run_command' returned malformed JSON arguments: {}",
            serde_json::from_str::<serde_json::Value>(arguments_raw).unwrap_err()
        );
        let prep = PreparedCall {
            call_id: "provider-malformed-id".into(),
            function_name: "run_command".into(),
            arguments_raw: arguments_raw.into(),
            ui_id: "turn_1_stream_1".into(),
            args: serde_json::json!({}),
            malformed_error: Some(error.clone()),
        };
        let original_tool = ToolCallItem {
            id: prep.ui_id.clone(),
            name: prep.function_name.clone(),
            args: serde_json::json!({"raw": arguments_raw}),
            status: "error".into(),
            result: Some(error.clone()),
            arguments_raw: Some(arguments_raw.into()),
            started_at: Some(1),
            completed_at: Some(1),
            duration_ms: Some(0),
            ..Default::default()
        };
        // Persiapan sudah menerbitkan error terminal sebelum antrean dibatalkan.
        emitter.tool(original_tool.clone());
        let prepared_events = events.0.lock().unwrap().clone();
        let data: SessionData = serde_json::from_value(serde_json::json!({
            "session": {"id": session_id, "title": "Terminal cancellation", "createdAt": 1, "updatedAt": 1, "messageCount": 2},
            "messages": [
                {"id": "user", "role": "user", "content": "question", "timestamp": 1},
                {"id": "assistant", "role": "assistant", "content": "", "timestamp": 2,
                 "runId": emitter.run_id, "isStreaming": true}
            ],
            "artifacts": []
        }))
        .unwrap();
        storage::save_session(data).await.unwrap();
        let mut page = storage::load_session_page(&session_id, None, 2, None)
            .await
            .unwrap();
        page.data.messages[1].tool_calls = Some(vec![
            serde_json::from_value(prepared_events[0]["data"]["tool"].clone()).unwrap(),
        ]);
        storage::checkpoint_session_delta(
            &session_id,
            CheckpointDelta {
                run_id: emitter.run_id.clone(),
                base_revision: page.window.revision.clone(),
                sequence: 1,
                messages: vec![page.data.messages[1].clone()],
                artifacts: vec![],
            },
        )
        .await
        .unwrap();
        let path = root.join(format!("{session_id}.json"));
        assert!(session_journal::path_for(&path).exists());
        assert!(session_journal::is_active(&path));

        let (call_id, execution) = cancelled_call(prep, &emitter);
        let final_events = events.0.lock().unwrap().clone();
        page.data.messages[1].tool_calls = Some(vec![
            serde_json::from_value(final_events.last().unwrap()["data"]["tool"].clone()).unwrap(),
        ]);
        page.data.messages[1].is_streaming = Some(false);
        page.data.messages[1].run_status = Some("cancelled".into());
        let saved = storage::save_session_page(page.data, page.window).await;
        eprintln!(
            "prepared_events={}, final_events={}, result_cancelled={}, final_save={:?}",
            prepared_events.len(),
            final_events.len(),
            execution.cancelled,
            saved.as_ref().map(|_| ()),
        );
        assert_eq!(
            final_events, prepared_events,
            "cancellation must not emit an overwrite for a terminal malformed error"
        );
        assert_eq!(call_id, "provider-malformed-id");
        assert_eq!(execution.output, error);
        assert!(!execution.success);
        assert!(!execution.cancelled);
        saved.expect("the checkpointed malformed error must allow finalization");
        let stored = storage::load_session(&session_id).await.unwrap();
        assert_eq!(
            serde_json::to_value(&stored.messages[1].tool_calls.as_ref().unwrap()[0]).unwrap(),
            serde_json::to_value(original_tool).unwrap(),
        );
        assert_eq!(stored.messages[1].is_streaming, Some(false));
        assert_eq!(stored.messages[1].run_status.as_deref(), Some("cancelled"));
        assert!(!session_journal::path_for(&path).exists());
        assert!(!session_journal::is_active(&path));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runner_terminal_cancel_valid_queued_calls_emit_cancelled_in_provider_order() {
        let events = ToolEventCollector::default();
        let emitter = EmitterContext {
            app: AgentHost::new(events.clone()),
            run_id: "queued-run".into(),
            session_id: "queued-session".into(),
        };
        let count = TOOL_BATCH_CONCURRENCY * 2;
        let calls = (0..count).map(|index| {
            let args = serde_json::json!({"command": format!("queued-{index}")});
            PreparedCall {
                call_id: format!("provider-id-{index}"),
                function_name: "run_command".into(),
                arguments_raw: args.to_string(),
                ui_id: format!("turn_1_stream_{index}"),
                args,
                malformed_error: None,
            }
        });
        let cancel = CancellationToken::new();
        cancel.cancel();
        let results = dispatch_tool_batch(
            calls,
            &cancel,
            |_| async { panic!("cancelled queued calls must not dispatch") },
            |prep| cancelled_call(prep, &emitter),
        )
        .await;
        let emitted = events.0.lock().unwrap();
        assert_eq!(results.len(), count);
        assert_eq!(emitted.len(), count);
        for (index, ((call_id, execution), event)) in results.iter().zip(emitted.iter()).enumerate() {
            assert_eq!(call_id, &format!("provider-id-{index}"));
            assert_eq!(execution.output, "Cancelled by operator");
            assert!(execution.cancelled);
            assert!(!execution.success);
            assert_eq!(event["type"], "tool_update");
            assert_eq!(event["data"]["runId"], emitter.run_id);
            assert_eq!(event["data"]["sessionId"], emitter.session_id);
            let tool = &event["data"]["tool"];
            let args = serde_json::json!({"command": format!("queued-{index}")});
            assert_eq!(tool["id"], format!("turn_1_stream_{index}"));
            assert_eq!(tool["name"], "run_command");
            assert_eq!(tool["args"], args);
            assert_eq!(tool["argumentsRaw"], args.to_string());
            assert_eq!(tool["status"], "cancelled");
            assert_eq!(tool["result"], "Cancelled by operator");
            assert!(tool["completedAt"].is_i64());
            assert_eq!(tool["durationMs"], 0);
        }
    }

    #[test]
    fn runner_concurrency_cancellation_skips_queued_side_effects_but_returns_every_id() {
        use std::future::Future;
        use std::task::{Context, Poll};

        for initially_cancelled in [false, true] {
            let count = TOOL_BATCH_CONCURRENCY * 3;
            let cancel = CancellationToken::new();
            if initially_cancelled {
                cancel.cancel();
            }
            let dispatched = Mutex::new(Vec::new());
            let side_effects = Mutex::new(Vec::new());
            let (senders, receivers): (Vec<_>, Vec<_>) =
                (0..count).map(|_| tokio::sync::oneshot::channel()).unzip();
            let mut senders: Vec<_> = senders.into_iter().map(Some).collect();
            let mut batch = Box::pin(dispatch_tool_batch(
                receivers.into_iter().enumerate(),
                &cancel,
                |(index, gate)| {
                    dispatched.lock().unwrap().push(index);
                    let side_effects = &side_effects;
                    async move {
                        side_effects.lock().unwrap().push(index);
                        gate.await.unwrap();
                        (format!("provider-id-{index}"), ToolExecutionResult::success(format!("completed-{index}"), None))
                    }
                },
                |(index, _)| {
                    (format!("provider-id-{index}"), ToolExecutionResult {
                        output: "Cancelled by operator".into(),
                        cancelled: true,
                        ..Default::default()
                    })
                },
            ));
            let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
            let mut state = batch.as_mut().poll(&mut cx);
            let admitted = if initially_cancelled { 0 } else { TOOL_BATCH_CONCURRENCY };
            if !initially_cancelled {
                assert!(state.is_pending());
                assert_eq!(dispatched.lock().unwrap().len(), admitted);
                cancel.cancel();
                for index in (0..admitted).rev() {
                    senders[index].take().unwrap().send(()).unwrap();
                }
                state = batch.as_mut().poll(&mut cx);
            }
            eprintln!("initially_cancelled={initially_cancelled}, dispatched={}, side_effects={}, admitted={admitted}", dispatched.lock().unwrap().len(), side_effects.lock().unwrap().len());
            assert_eq!(
                dispatched.lock().unwrap().len(), admitted,
                "cancellation must not invoke dispatch closures for queued calls"
            );
            assert_eq!(side_effects.lock().unwrap().len(), admitted);
            let Poll::Ready(results) = state else {
                panic!("cancelled queued calls must drain without waiting on their gates");
            };
            assert_eq!(results.len(), count);
            for (index, (call_id, result)) in results.iter().enumerate() {
                assert_eq!(call_id, &format!("provider-id-{index}"));
                assert_eq!(result.cancelled, index >= admitted);
                assert_eq!(result.success, index < admitted);
                assert_eq!(result.output, if index < admitted { format!("completed-{index}") } else { "Cancelled by operator".into() });
            }
        }
    }

    #[test]
    fn runner_concurrency_bounds_active_calls_and_preserves_provider_result_order() {
        use std::future::Future;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::task::{Context, Poll};

        let count = TOOL_BATCH_CONCURRENCY * 3;
        let cancel = CancellationToken::new();
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let started = Mutex::new(Vec::new());
        let completed = Mutex::new(Vec::new());
        let (senders, receivers): (Vec<_>, Vec<_>) =
            (0..count).map(|_| tokio::sync::oneshot::channel()).unzip();
        let mut senders: Vec<_> = senders.into_iter().map(Some).collect();
        let mut batch = Box::pin(dispatch_tool_batch(
            receivers.into_iter().enumerate(),
            &cancel,
            |(index, gate)| {
                started.lock().unwrap().push(index);
                let (active, peak, completed) = (&active, &peak, &completed);
                async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(current, Ordering::SeqCst);
                    gate.await.unwrap();
                    active.fetch_sub(1, Ordering::SeqCst);
                    completed.lock().unwrap().push(index);
                    let result = if index % 3 == 1 {
                        ToolExecutionResult::failure(format!("denied-{index}"))
                    } else {
                        ToolExecutionResult::success(format!("result-{index}"), None)
                    };
                    (format!("provider-id-{index}"), result)
                }
            },
            |(index, _)| (format!("provider-id-{index}"), ToolExecutionResult::default()),
        ));
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        assert!(batch.as_mut().poll(&mut cx).is_pending());
        eprintln!(
            "prepared={count}, dispatched={}, active={}, limit={TOOL_BATCH_CONCURRENCY}",
            started.lock().unwrap().len(),
            active.load(Ordering::SeqCst)
        );
        assert_eq!(
            started.lock().unwrap().len(),
            TOOL_BATCH_CONCURRENCY,
            "queued dispatch closures must remain uninvoked until a batch slot is free"
        );
        assert_eq!(active.load(Ordering::SeqCst), TOOL_BATCH_CONCURRENCY);

        let mut results = None;
        for offset in (0..count).step_by(TOOL_BATCH_CONCURRENCY) {
            for index in (offset..offset + TOOL_BATCH_CONCURRENCY).rev() {
                senders[index].take().unwrap().send(()).unwrap();
                if let Poll::Ready(value) = batch.as_mut().poll(&mut cx) {
                    assert_eq!(index, count - TOOL_BATCH_CONCURRENCY);
                    results = Some(value);
                }
            }
        }
        let results = results.expect("all gated calls must finish");
        assert_eq!(results.len(), count);
        for (index, (call_id, result)) in results.iter().enumerate() {
            assert_eq!(call_id, &format!("provider-id-{index}"));
            assert_eq!(result.success, index % 3 != 1);
            assert_eq!(
                result.output,
                if result.success { format!("result-{index}") } else { format!("denied-{index}") }
            );
        }
        let expected_completion: Vec<_> = (0..count)
            .step_by(TOOL_BATCH_CONCURRENCY)
            .flat_map(|offset| (offset..offset + TOOL_BATCH_CONCURRENCY).rev())
            .collect();
        assert_eq!(*completed.lock().unwrap(), expected_completion);
        assert_eq!(peak.load(Ordering::SeqCst), TOOL_BATCH_CONCURRENCY);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}
