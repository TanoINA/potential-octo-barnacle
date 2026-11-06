use super::mcp::{result_text, McpManager};
use super::provider::{stream_chat_completion, CompletionOptions, ToolDelta};
use super::skills::{
    find_skill, load_skills, read_skill_content, render_skill_catalog, workspace_skill_paths,
};
use super::tools::{execute_tool, get_agent_tools, ToolContext, ToolEvent, ToolExecutionResult};
use super::types::{
    AgentEvent, AgentRunRequest, Artifact, ArtifactEventData, ChatMessage, CouncilChunkData,
    CouncilDoneData, DoneEventData, ErrorEventData, MicroCompactionEvent, ProviderConfig,
    ReasoningEffort, SkillActivatedEventData, SkillInfo, StreamEventData, SubagentEventData,
    SubagentProgress, TerminalEventData, ToolCallItem, ToolEventData,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
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
                "Skill '{}' activated.\n\nInstructions:\n{}",
                skill.name, instructions
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
) -> SubagentToolOutcome {
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
    let mut target_dir = if workspace_cwd.trim().is_empty() {
        std::env::temp_dir()
    } else {
        match super::tools::resolve_workspace_path(workspace_cwd, ".") {
            Ok(path) => path,
            Err(error) => return ToolExecutionResult::failure(error),
        }
    };
    if let Some(sub) = subpath {
        if workspace_cwd.trim().is_empty() {
            return ToolExecutionResult::failure(
                "Subagent subpath requires an active workspace".into(),
            );
        }
        match super::tools::resolve_workspace_path(workspace_cwd, sub) {
            Ok(candidate) if candidate.is_dir() => target_dir = candidate,
            Ok(_) => {
                return ToolExecutionResult::failure(
                    "Subagent subpath must be an existing directory inside the workspace".into(),
                )
            }
            Err(error) => return ToolExecutionResult::failure(error),
        }
    }
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

    let mut subagent_system = format!(
        "You are an autonomous subagent specialized as '{role}'.\nWorking directory: {cwd_str}\nTask: {task}\n\nInstructions:\n- Investigate and execute the assigned task using the available tools.\n- Keep intermediate tool call explanations brief.\n- Once all tool steps are complete, conclude with a comprehensive, standalone final report summarizing all your steps, findings, and conclusions. Do not call any tools when providing your final report."
    );
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

    let options = CompletionOptions {
        thinking: false,
        reasoning_effort: ReasoningEffort::Low,
        temperature: 0.5,
        max_tokens: Some(8192),
        stream: true,
    };

    let sub_context = ToolContext {
        cwd: cwd_str,
        cancel: parent_cancel.clone(),
        sink: None,
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
        let result = stream_chat_completion(
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

        let result = match result {
            Ok(r) => r,
            Err(err) => {
                progress.status = "failed".into();
                progress.completed_at = Some(chrono::Utc::now().timestamp_millis());
                progress.summary = Some(format!("Error: {err}"));
                emitter.subagent(progress);
                return ToolExecutionResult::failure(format!(
                    "Subagent encountered provider error: {err}"
                ));
            }
        };

        if result.tool_calls.is_empty() {
            final_text = turn_text;
            completed_cleanly = true;
            break;
        }

        if is_substantive_finding(&turn_text) {
            intermediate_findings.push(turn_text.trim().to_string());
        }

        let mut assistant_calls = Vec::new();
        for call in &result.tool_calls {
            assistant_calls.push(serde_json::json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.function.name,
                    "arguments": call.function.arguments.clone()
                }
            }));
        }
        conversation.push(serde_json::json!({
            "role": "assistant",
            "content": turn_text,
            "tool_calls": assistant_calls
        }));

        for call in &result.tool_calls {
            let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                .unwrap_or_else(|_| serde_json::json!({}));
            let outcome = dispatch_subagent_tool(
                &call.function.name,
                &args,
                &sub_context,
                &mut skills,
                &all_skill_paths,
            )
            .await;
            if let Some(skill) = outcome.activated_skill {
                emitter.skill(skill);
            }
            conversation.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": outcome.result.output
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
        if completed_cleanly && !final_text.trim().is_empty() {
            conversation.push(serde_json::json!({
                "role": "assistant",
                "content": final_text
            }));
        }

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
        let synth_options = CompletionOptions {
            thinking: false,
            reasoning_effort: ReasoningEffort::Low,
            temperature: 0.3,
            max_tokens: Some(8192),
            stream: true,
        };

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

        if synth_res.is_ok() && !synthesis_text.trim().is_empty() {
            final_text = synthesis_text;
        }
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
        let mcp = tokio::select! {
            _ = cancel.cancelled() => {
                emitter.done(true, false);
                return;
            }
            manager = McpManager::connect(&request.mcp_servers, &workspace_cwd) => manager,
        };
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
        let mut skills = load_skills(&all_skill_paths).await;
        let default_prompt = "You are AetherAgent, a native expert developer assistant. Follow these strict guidelines:\n- Maintain a clean, minimalist developer aesthetic (like Cursor and Linear).\n- CRITICAL: Never use emojis, emotes, smileys, decorative symbols, or gamified AI tropes in responses.\n- Keep explanations concise, clear, and direct without conversational filler or chatbot pleasantries.\n- Inspect before editing, use native tools deliberately, and route long outputs to artifacts.";
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
        match load_workspace_memory(&workspace_cwd).await {
            Ok(Some(memory)) => {
                system.push_str("\n## Workspace AGENTS.md instructions\n");
                system.push_str(&memory);
            }
            Ok(None) => {}
            Err(error) => emitter.error(error),
        }
        system.push_str(&format!("\n## Active workspace\n{workspace_cwd}"));

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
            let mut partial_tools: HashMap<usize, ToolCallItem> = HashMap::new();
            let chunk_emitter = emitter.clone();
            let reasoning_emitter = emitter.clone();
            let tool_emitter = emitter.clone();
            let result = stream_chat_completion(
                &request.provider,
                &serde_json::Value::Array(conversation.clone()),
                &tools,
                &options,
                move |chunk| chunk_emitter.chunk(chunk),
                move |reasoning| reasoning_emitter.reasoning(reasoning),
                move |delta: ToolDelta| {
                    let item = partial_tools
                        .entry(delta.index)
                        .or_insert_with(|| ToolCallItem {
                            id: format!("turn_{turns}_stream_{}", delta.index),
                            name: "tool".into(),
                            args: serde_json::Value::Object(Default::default()),
                            status: "pending".into(),
                            result: None,
                            artifact_id: None,
                            arguments_raw: Some(String::new()),
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

            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    emitter.error(redact_provider_error(error, &request.provider));
                    break;
                }
            };
            if result.cancelled || cancel.is_cancelled() {
                break;
            }
            if result.tool_calls.is_empty() {
                conversation
                    .push(serde_json::json!({"role":"assistant", "content":result.content}));
                finished = true;
                break;
            }
            used_tools = true;
            conversation.push(serde_json::json!({
                "role": "assistant",
                "content": if result.content.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(result.content) },
                "tool_calls": serde_json::to_value(&result.tool_calls).unwrap_or(serde_json::Value::Null),
            }));

            for call in result.tool_calls.into_iter() {
                if cancel.is_cancelled() {
                    break;
                }
                let ui_id = format!("turn_{turns}_stream_{}", call.stream_index);
                let parsed_args = if call.function.arguments.trim().is_empty() {
                    Ok(serde_json::json!({}))
                } else {
                    serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                        .map_err(|error| error.to_string())
                };
                let args = match parsed_args {
                    Ok(args) => args,
                    Err(error) => {
                        let message = format!(
                            "Tool '{}' returned malformed JSON arguments: {error}",
                            call.function.name
                        );
                        emitter.tool(ToolCallItem {
                            id: ui_id,
                            name: call.function.name.clone(),
                            args: serde_json::json!({ "raw": call.function.arguments.clone() }),
                            status: "error".into(),
                            result: Some(message.clone()),
                            artifact_id: None,
                            arguments_raw: Some(call.function.arguments.clone()),
                            ..Default::default()
                        });
                        conversation.push(serde_json::json!({"role":"tool","tool_call_id":call.id,"content":message}));
                        continue;
                    }
                };
                let mut tool_item = ToolCallItem {
                    id: ui_id,
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
                emitter.tool(tool_item.clone());

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
                    cwd: workspace_cwd.clone(),
                    cancel: cancel.clone(),
                    sink: Some(sink),
                };
                let mut execution = if mcp.contains(&call.function.name) {
                    let outcome = tokio::select! {
                        _ = cancel.cancelled() => Err("MCP call cancelled".to_string()),
                        result = mcp.call(&call.function.name, args.clone()) => result,
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
                                        title: format!("MCP: {}", call.function.name),
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
                } else if call.function.name == "use_skill" {
                    let skill_name = args
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let activation =
                        activate_skill(&mut skills, skill_name, &all_skill_paths).await;
                    if let Some(skill) = activation.skill {
                        emitter.skill(skill);
                    }
                    activation.result
                } else if call.function.name == "spawn_subagent" {
                    let role = args
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Specialist");
                    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("");
                    let subpath = args.get("subpath").and_then(|v| v.as_str());
                    let params = SubagentParams {
                        role,
                        task,
                        subpath,
                        skill_paths: &request.skill_paths,
                    };
                    run_subagent(
                        &emitter,
                        &request.provider,
                        &workspace_cwd,
                        params,
                        cancel.clone(),
                    )
                    .await
                } else {
                    let exec = execute_tool(&call.function.name, &args, &context).await;
                    if call.function.name == "learn_skill" && exec.success {
                        skills = load_skills(&all_skill_paths).await;
                    }
                    if call.function.name == "read_file" {
                        if let Some(file_path) = args
                            .get("filePath")
                            .or_else(|| args.get("file_path"))
                            .and_then(|v| v.as_str())
                        {
                            if file_path.ends_with("SKILL.md") {
                                for s in &skills {
                                    if file_path.contains(&s.name) || s.path.ends_with(file_path) {
                                        emitter.skill(s.clone());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    exec
                };
                if let Some(artifact) = execution.artifact.take() {
                    tool_item.artifact_id = Some(artifact.id.clone());
                    emitter.artifact(artifact);
                }
                tool_item.status = if execution.cancelled {
                    "cancelled"
                } else if execution.success {
                    "done"
                } else {
                    "error"
                }
                .into();
                tool_item.result = Some(execution.output.clone());
                tool_item.completed_at = Some(chrono::Utc::now().timestamp_millis());
                tool_item.duration_ms = execution.duration_ms.or_else(|| {
                    tool_item
                        .started_at
                        .map(|s| (chrono::Utc::now().timestamp_millis() - s).max(0) as u64)
                });
                tool_item.exit_code = execution.exit_code;
                tool_item.terminal_output = execution.terminal_output;
                tool_item.files_changed = execution.files_changed;
                emitter.tool(tool_item);
                conversation.push(serde_json::json!({"role":"tool","tool_call_id":call.id,"content":execution.output}));
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

        let default_prompt = "You are AetherAgent, an expert developer assistant. Follow these strict guidelines:\n- Maintain a clean, minimalist developer aesthetic.\n- Never use emojis, emotes, smileys, decorative symbols, or gamified AI tropes in responses.\n- Provide a direct, comprehensive, and high-quality response.";
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
                            call.result = Some(replacement);
                            pruned_count += 1;
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

fn append_history_message(conversation: &mut Vec<serde_json::Value>, message: &ChatMessage) {
    match message.role.as_str() {
        "user" => conversation.push(serde_json::json!({"role":"user","content":message.content})),
        "system" => {
            conversation.push(serde_json::json!({"role":"system","content":message.content}))
        }
        "assistant" => {
            let calls = message.tool_calls.as_ref().map(|items| items.iter().map(|item| serde_json::json!({
                "id": item.id, "type": "function",
                "function": {"name":item.name,"arguments":if item.args.is_string(){item.args.as_str().unwrap_or("{}").to_string()}else{item.args.to_string()}},
            })).collect::<Vec<_>>()).unwrap_or_default();
            conversation.push(serde_json::json!({
                "role":"assistant", "content":message.content,
                "tool_calls":if calls.is_empty(){serde_json::Value::Null}else{serde_json::Value::Array(calls)},
            }));
            if let Some(items) = &message.tool_calls {
                for item in items {
                    if let Some(result) = &item.result {
                        conversation.push(serde_json::json!({"role":"tool","tool_call_id":item.id,"content":result}));
                    }
                }
            }
        }
        _ => {}
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
    use super::*;

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

        let event = apply_micro_compaction(&mut messages, "r1", "s1", 50);
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
        let root = std::env::temp_dir().join(format!("aether-runner-{}", Uuid::new_v4()));
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
}
