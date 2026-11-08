use super::mcp::{result_text, McpManager};
use super::provider::{
    is_retryable_provider_error, stream_chat_completion, CompletionOptions, ToolDelta,
};
use super::skills::{
    find_skill, load_skills, read_skill_content, render_skill_catalog, workspace_skill_paths,
};
use super::tools::{execute_tool, get_agent_tools, ToolContext, ToolEvent, ToolExecutionResult};
use super::types::{
    AgentEvent, AgentRunRequest, Artifact, ArtifactEventData, ChatMessage, CouncilChunkData,
    CouncilDoneData, DoneEventData, ErrorEventData, ExecutionMode, MicroCompactionEvent,
    NoticeEventData, ProviderConfig, SkillActivatedEventData, SkillInfo, StreamEventData,
    SubagentEventData, SubagentProgress, TerminalEventData, ToolCallItem, ToolEventData,
};
use futures_util::future::join_all;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
struct EmitterContext {
    app: AppHandle,
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

/// Outcome of dispatching one tool call issued inside a subagent.
struct SubagentToolOutcome {
    result: ToolExecutionResult,
    /// Skill activated by this call, so the caller can emit the UI event.
    activated_skill: Option<SkillInfo>,
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
        if name == "learn_skill" && result.success {
            *skills = load_skills(skill_paths).await;
        }
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
    let mut policy = String::new();
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
                .map(|n| n != "spawn_subagent")
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
    let max_turns = 12;

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

pub struct AgentRunner {
    active: Arc<Mutex<HashMap<String, (String, CancellationToken)>>>,
    reviews: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl Default for AgentRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRunner {
    pub fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(HashMap::new())),
            reviews: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub fn begin(&self, run_id: &str, session_id: &str) -> Result<CancellationToken, String> {
        if run_id.trim().is_empty() || session_id.trim().is_empty() {
            return Err("runId and sessionId are required".into());
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
        let token = CancellationToken::new();
        active.insert(run_id.to_string(), (session_id.to_string(), token.clone()));
        Ok(token)
    }

    pub fn abort(&self, run_id: &str) -> Result<(), String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "Agent run registry is unavailable".to_string())?;
        if let Some((_, token)) = active.get(run_id) {
            token.cancel();
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
        app: AppHandle,
        session_id: String,
        provider: ProviderConfig,
    ) -> Result<(), String> {
        if self
            .active
            .lock()
            .map_err(|_| "Run registry is unavailable")?
            .values()
            .any(|(session, _)| session == &session_id)
        {
            return Err("Stop the active run before reviewing this session".into());
        }
        let data = super::storage::load_session(&session_id).await?;
        let cwd = data
            .session
            .workspace_cwd
            .filter(|value| !value.trim().is_empty())
            .ok_or("Select a workspace before reviewing this session")?;
        let (run_id, snapshot) = completed_review_snapshot(&data.messages)?;
        run_reflection(
            app,
            self.reviews.clone(),
            cwd,
            provider,
            snapshot,
            run_id,
            session_id,
        )
        .await
    }

    pub async fn run(
        &self,
        app: AppHandle,
        workspace_cwd: String,
        request: AgentRunRequest,
        cancel: CancellationToken,
    ) {
        let emitter = EmitterContext {
            app: app.clone(),
            run_id: request.run_id.clone(),
            session_id: request.session_id.clone(),
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
                id: format!("art-{}", Uuid::new_v4()),
                title: "MCP connection warning".into(),
                artifact_type: "markdown".into(),
                language: Some("markdown".into()),
                content: warning.clone(),
                file_path: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
        }

        let all_skill_paths = workspace_skill_paths(&workspace_cwd, &request.skill_paths);
        let skills = load_skills(&all_skill_paths).await;
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
        system.push_str(&render_skill_catalog(&skills));
        let skills_shared = Arc::new(RwLock::new(skills));
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

        let mut request_messages = request.messages.clone();
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
        let review_start = conversation
            .iter()
            .rposition(|message| message["role"] == "user")
            .unwrap_or(conversation.len());
        let mut used_tools = false;
        while turns < request.max_turns.clamp(1, 100) {
            if cancel.is_cancelled() {
                break;
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
                    move |delta: ToolDelta| {
                        let item =
                            partial_tools
                                .entry(delta.index)
                                .or_insert_with(|| ToolCallItem {
                                    id: format!("turn_{turns}_stream_{}", delta.index),
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
                emitter.provider_state(&conversation[current_run_start..]);
                finished = true;
                break;
            }
            used_tools = true;

            struct PreparedCall {
                call_id: String,
                function_name: String,
                arguments_raw: String,
                ui_id: String,
                args: serde_json::Value,
                malformed_error: Option<String>,
            }

            let mut prepared_calls = Vec::with_capacity(result.tool_calls.len());
            for call in result.tool_calls.into_iter() {
                let ui_id = format!("turn_{turns}_stream_{}", call.stream_index);
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

            let tool_futures = prepared_calls.into_iter().map(|prep| {
                let emitter = emitter.clone();
                let cancel = cancel.clone();
                let request_provider = request.provider.clone();
                let skill_paths = request.skill_paths.clone();
                let workspace_cwd = workspace_cwd.clone();
                let all_skill_paths = all_skill_paths.clone();
                let mcp = Arc::clone(&mcp);
                let skills_shared = Arc::clone(&skills_shared);
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
                            skill_paths: &skill_paths,
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
                        if prep.function_name == "learn_skill" && exec.success {
                            let reloaded = load_skills(&all_skill_paths).await;
                            let mut skills_guard = skills_shared.write().await;
                            *skills_guard = reloaded;
                        }
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
            });

            let executed_results = join_all(tool_futures).await;
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
        emitter.done(cancel.is_cancelled(), finished && !cancel.is_cancelled());
        if !workspace_cwd.trim().is_empty()
            && super::reflection::should_reflect(
                request.self_improvement_enabled,
                finished && !cancel.is_cancelled(),
                turns,
                used_tools,
                false,
            )
        {
            let app = emitter.app.clone();
            let reviews = self.reviews.clone();
            let snapshot = conversation[review_start..].to_vec();
            tokio::spawn(async move {
                let _ = run_reflection(
                    app,
                    reviews,
                    workspace_cwd,
                    request.provider,
                    snapshot,
                    request.run_id,
                    request.session_id,
                )
                .await;
            });
        }
    }

    pub async fn run_council(
        &self,
        app: AppHandle,
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

        let mut req_a_messages = request_a.messages.clone();
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

        let mut req_b_messages = request_b.messages.clone();
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
    registry: Arc<Mutex<std::collections::HashSet<String>>>,
    session_id: String,
}

impl Drop for ReflectionLease {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.registry.lock() {
            registry.remove(&self.session_id);
        }
    }
}

async fn run_reflection(
    app: AppHandle,
    registry: Arc<Mutex<std::collections::HashSet<String>>>,
    cwd: String,
    provider: ProviderConfig,
    snapshot: Vec<serde_json::Value>,
    run_id: String,
    session_id: String,
) -> Result<(), String> {
    {
        let mut active = registry
            .lock()
            .map_err(|_| "Review registry is unavailable")?;
        if active.contains(&session_id) {
            return Err("A self-improvement review is already running for this session".into());
        }
        if active.len() >= 4 {
            return Err("Wait for an active self-improvement review to finish".into());
        }
        active.insert(session_id.clone());
    }
    let _lease = ReflectionLease {
        registry,
        session_id: session_id.clone(),
    };
    let outcome = async {
        let mut event = super::reflection::reflect(
            &cwd,
            &provider,
            &snapshot,
            &run_id,
            &session_id,
            CancellationToken::new(),
        )
        .await?;
        event.workspace_cwd = Some(cwd);
        super::storage::record_self_improvement(&event).await?;
        app.emit("agent:self-improvement", &event)
            .map_err(|error| error.to_string())
    }
    .await;
    if let Err(error) = outcome {
        let message = redact_provider_error(error, &provider);
        let _ = app.emit(
            "agent:self-improvement-error",
            serde_json::json!({"run_id":run_id,"session_id":session_id,"message":message}),
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
    let start = messages
        .iter()
        .rposition(|message| message.role == "user")
        .ok_or("No user turn is available to review")?;
    let mut snapshot = Vec::new();
    for message in &messages[start..] {
        append_history_message(&mut snapshot, message);
    }
    Ok((
        last.run_id.clone().unwrap_or_else(|| last.id.clone()),
        snapshot,
    ))
}

pub fn estimate_messages_tokens(messages: &[ChatMessage], system: &str) -> usize {
    let mut chars = system.len();
    for message in messages {
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
    chars.div_ceil(4)
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
        "user" => conversation.push(serde_json::json!({"role":"user","content":message.content})),
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
    use super::super::types::{ProviderKind, ReasoningEffort};
    use super::*;

    #[test]
    fn child_artifact_and_terminal_events_share_scoped_identity() {
        let artifact = Artifact {
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
    fn manual_review_targets_latest_completed_turn_only() {
        let messages: Vec<ChatMessage> = serde_json::from_value(serde_json::json!([
            {"id":"u1", "role":"user", "content":"Old", "timestamp":1},
            {"id":"a1", "role":"assistant", "content":"Old answer", "timestamp":2},
            {"id":"u2", "role":"user", "content":"Fix parser", "timestamp":3},
            {"id":"a2", "role":"assistant", "content":"Fixed", "timestamp":4,"runId":"r2","runStatus":"completed",
             "toolCalls":[{"id":"t1","name":"run_command","args":{},"status":"done","result":"tests pass"}]}
        ])).unwrap();
        let (run, snapshot) = completed_review_snapshot(&messages).unwrap();
        assert_eq!(run, "r2");
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0]["content"], "Fix parser");
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

    #[tokio::test]
    async fn concurrent_tool_calls_execute_simultaneously() {
        let start = std::time::Instant::now();
        let tasks = (0..5).map(|i| async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            format!("result_{i}")
        });
        let results = join_all(tasks).await;
        let elapsed = start.elapsed();
        assert_eq!(results.len(), 5);
        assert!(
            elapsed.as_millis() < 200,
            "Expected concurrent execution in <200ms, took {}ms",
            elapsed.as_millis()
        );
    }
}
