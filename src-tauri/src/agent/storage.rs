use super::types::{
    AgentSettings, ChatMessage, GlobalArtifactItem, McpServerConfig, ProviderConfig, ProviderKind,
    Session, SessionData,
};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;

static SESSION_IO_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aether-agent")
}

fn sessions_dir() -> PathBuf {
    config_dir().join("sessions")
}
fn settings_path() -> PathBuf {
    config_dir().join("config.json")
}
fn workspace_path() -> PathBuf {
    config_dir().join("workspace.json")
}

fn provider(
    id: &str,
    name: &str,
    kind: ProviderKind,
    base_url: &str,
    model: &str,
) -> ProviderConfig {
    ProviderConfig {
        id: id.into(),
        name: name.into(),
        kind,
        base_url: base_url.into(),
        model: model.into(),
        ..ProviderConfig::default()
    }
}

pub fn default_settings() -> AgentSettings {
    let providers = vec![
        provider(
            "gutsai",
            "GutsAI",
            ProviderKind::Openai,
            "https://api.gutsai.id/v1",
            "kimi-k2-thinking",
        ),
        provider(
            "anthropic",
            "Anthropic",
            ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-sonnet-4-5",
        ),
        provider(
            "deepseek",
            "DeepSeek",
            ProviderKind::Deepseek,
            "https://api.deepseek.com/v1",
            "deepseek-chat",
        ),
        provider(
            "ollama",
            "Ollama (Local)",
            ProviderKind::Ollama,
            "http://localhost:11434/v1",
            "llama3.2",
        ),
    ];
    AgentSettings {
        active_provider_id: "gutsai".into(),
        providers,
        ..AgentSettings::default()
    }
}

pub async fn load_settings() -> Result<AgentSettings, String> {
    let defaults = default_settings();
    let content = match fs::read_to_string(settings_path()).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            save_settings(&defaults).await?;
            return Ok(defaults);
        }
        Err(error) => return Err(format!("Could not read settings: {error}")),
    };
    let mut value = serde_json::from_str::<serde_json::Value>(&content).map_err(|error| {
        format!("Settings JSON is invalid; the original file was preserved: {error}")
    })?;
    migrate_provider_kinds(&mut value);
    let mut settings = serde_json::from_value::<AgentSettings>(value).map_err(|error| {
        format!("Settings schema is invalid; the original file was preserved: {error}")
    })?;
    if settings.providers.is_empty() {
        settings.providers = defaults.providers;
    }
    if !settings
        .providers
        .iter()
        .any(|provider| provider.id == settings.active_provider_id)
    {
        settings.active_provider_id = settings.providers[0].id.clone();
    }
    settings.max_turns = settings.max_turns.clamp(1, 100);
    if !settings.temperature.is_finite() {
        settings.temperature = 0.7;
    }
    Ok(settings)
}

fn migrate_provider_kinds(value: &mut serde_json::Value) {
    let Some(providers) = value
        .get_mut("providers")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    for provider in providers {
        if provider.get("kind").is_some() {
            continue;
        }
        let id = provider
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let base = provider
            .get("baseUrl")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let kind = if id.contains("ollama") || base.contains("11434") {
            "ollama"
        } else if id.contains("deepseek") || base.contains("deepseek") {
            "deepseek"
        } else if id.contains("anthropic") || base.contains("anthropic") {
            "anthropic"
        } else {
            "openai"
        };
        provider["kind"] = kind.into();
    }
}

pub async fn save_settings(settings: &AgentSettings) -> Result<(), String> {
    if settings.providers.is_empty() {
        return Err("At least one provider is required".into());
    }
    if !settings.temperature.is_finite() || !(0.0..=2.0).contains(&settings.temperature) {
        return Err("Temperature must be between 0 and 2".into());
    }
    if !(1..=100).contains(&settings.max_turns) {
        return Err("Maximum agent turns must be between 1 and 100".into());
    }
    if settings.max_tokens == Some(0) {
        return Err("Max tokens must be at least 1 or left unset".into());
    }
    let mut ids = std::collections::HashSet::new();
    for provider in &settings.providers {
        validate_safe_id(&provider.id)?;
        if !ids.insert(&provider.id) {
            return Err(format!("Duplicate provider ID: {}", provider.id));
        }
        let url = Url::parse(&provider.base_url)
            .map_err(|error| format!("Invalid base URL for {}: {error}", provider.name))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(format!(
                "Provider {} requires an HTTP(S) base URL with a host",
                provider.name
            ));
        }
    }
    for server in &settings.mcp_servers {
        validate_mcp_server(server)?;
    }
    let content = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    atomic_write(&settings_path(), &content).await
}

fn validate_safe_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(
            "IDs must contain only ASCII letters, numbers, underscores, or hyphens (max 128 chars)"
                .into(),
        );
    }
    Ok(())
}

fn session_path(id: &str) -> Result<PathBuf, String> {
    validate_safe_id(id)?;
    Ok(sessions_dir().join(format!("{id}.json")))
}

pub async fn list_sessions() -> Vec<Session> {
    let _guard = SESSION_IO_LOCK.lock().await;
    let mut sessions = Vec::new();
    let Ok(mut entries) = fs::read_dir(sessions_dir()).await else {
        return sessions;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(path).await else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        if let Ok(data) = parse_session(value) {
            sessions.push(data.session);
        }
    }
    sessions.sort_by(|a, b| match (a.pinned, b.pinned) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (true, true) => {
            let a_pin = a.pinned_at.unwrap_or(a.updated_at);
            let b_pin = b.pinned_at.unwrap_or(b.updated_at);
            b_pin.cmp(&a_pin)
        }
        (false, false) => b.updated_at.cmp(&a.updated_at),
    });
    sessions
}

pub async fn list_all_artifacts() -> Vec<GlobalArtifactItem> {
    let _guard = SESSION_IO_LOCK.lock().await;
    let mut items = Vec::new();
    let Ok(mut entries) = fs::read_dir(sessions_dir()).await else {
        return items;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(path).await else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        if let Ok(data) = parse_session(value) {
            for art in data.artifacts {
                items.push(GlobalArtifactItem {
                    session_id: data.session.id.clone(),
                    session_title: data.session.title.clone(),
                    session_updated_at: data.session.updated_at,
                    artifact: art,
                });
            }
        }
    }
    items.sort_by_key(|a| std::cmp::Reverse(a.artifact.timestamp));
    items
}

pub async fn load_session(session_id: &str) -> Result<SessionData, String> {
    let _guard = SESSION_IO_LOCK.lock().await;
    load_session_unlocked(session_id).await
}

async fn load_session_unlocked(session_id: &str) -> Result<SessionData, String> {
    let path = session_path(session_id)?;
    let content = fs::read_to_string(&path)
        .await
        .map_err(|error| format!("Could not load session {session_id}: {error}"))?;
    let value = serde_json::from_str(&content)
        .map_err(|error| format!("Session JSON is invalid: {error}"))?;
    parse_session(value)
}

fn parse_session(value: serde_json::Value) -> Result<SessionData, String> {
    if value.get("session").is_some() {
        return serde_json::from_value(value)
            .map_err(|error| format!("Session schema is invalid: {error}"));
    }
    let session = Session {
        id: value
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        title: value
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or("Untitled")
            .to_string(),
        created_at: value
            .get("createdAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
        updated_at: value
            .get("updatedAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
        message_count: value
            .get("messages")
            .and_then(|value| value.as_array())
            .map(|messages| messages.len())
            .unwrap_or(0),
        workspace_cwd: value
            .get("workspaceCwd")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        preview: value
            .get("preview")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        pinned: value
            .get("pinned")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        pinned_at: value.get("pinnedAt").and_then(|value| value.as_i64()),
    };
    let messages = value
        .get("messages")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let artifacts = value
        .get("artifacts")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let model_override = value
        .get("modelOverride")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(None);
    Ok(SessionData {
        session,
        messages,
        artifacts,
        model_override,
    })
}

pub async fn save_session(data: SessionData) -> Result<(), String> {
    let _guard = SESSION_IO_LOCK.lock().await;
    save_session_unlocked(data).await
}

fn merge_self_improvements(data: &mut SessionData, previous: &SessionData) {
    for message in &mut data.messages {
        if let Some(stored) = previous
            .messages
            .iter()
            .find(|stored| stored.id == message.id && stored.run_id == message.run_id)
        {
            // Native review results win over stale frontend snapshots of the same turn.
            if stored.self_improvement.is_some() {
                message.self_improvement = stored.self_improvement.clone();
            }
        }
    }
}

pub async fn record_self_improvement(
    event: &super::types::SelfImprovementEvent,
) -> Result<(), String> {
    let _guard = SESSION_IO_LOCK.lock().await;
    let mut data = load_session_unlocked(&event.session_id).await?;
    let message = data
        .messages
        .iter_mut()
        .find(|message| {
            message.role == "assistant"
                && (message.run_id.as_deref() == Some(event.run_id.as_str())
                    || message.id == event.run_id)
        })
        .ok_or("The reviewed turn no longer exists in this session")?;
    message.self_improvement = Some(event.clone());
    let bytes = serde_json::to_vec_pretty(&data).map_err(|error| error.to_string())?;
    atomic_write(&session_path(&event.session_id)?, &bytes).await
}

/// Merge pin state dari sesi yang tersimpan di disk ke `data`.
/// `pin_is_authoritative == true` berarti pemanggil (mis. `toggle_pin_session`)
/// sudah menentukan state pin secara eksplisit — disk tidak boleh menimpanya,
/// sehingga pin yang baru dilepas tidak "hidup" lagi pada save berikutnya.
fn merge_pin_state(data: &mut Session, previous: &Session, pin_is_authoritative: bool) {
    if pin_is_authoritative {
        return;
    }
    if data.pinned_at.is_none() && previous.pinned {
        data.pinned = previous.pinned;
        data.pinned_at = previous.pinned_at;
    }
}

async fn save_session_unlocked(data: SessionData) -> Result<(), String> {
    save_session_unlocked_with_pin_policy(data, false).await
}

async fn save_session_unlocked_with_pin_policy(
    mut data: SessionData,
    pin_is_authoritative: bool,
) -> Result<(), String> {
    let path = session_path(&data.session.id)?;
    let now = chrono::Utc::now().timestamp_millis();
    if let Ok(previous) = load_session_unlocked(&data.session.id).await {
        data.session.created_at = previous.session.created_at;
        merge_pin_state(&mut data.session, &previous.session, pin_is_authoritative);
        merge_self_improvements(&mut data, &previous);
    }
    if data.session.created_at <= 0 {
        data.session.created_at = now;
    }
    data.session.updated_at = now;
    data.session.message_count = data.messages.len();
    data.session.title = bounded_title(&data.session.title);
    data.session.preview = session_preview(&data.messages);
    let content = serde_json::to_vec_pretty(&data).map_err(|error| error.to_string())?;
    atomic_write(&path, &content).await?;
    let _ = super::search::index_session(&data);
    Ok(())
}

fn bounded_title(title: &str) -> String {
    let trimmed = title.trim();
    let value = if trimmed.is_empty() {
        "New Chat"
    } else {
        trimmed
    };
    value.chars().take(200).collect()
}

fn session_preview(messages: &[ChatMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| {
            matches!(message.role.as_str(), "user" | "assistant")
                && !message.content.trim().is_empty()
        })
        .map(|message| {
            message
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(240)
                .collect()
        })
}

pub async fn rename_session(session_id: &str, title: &str) -> Result<SessionData, String> {
    if title.trim().is_empty() {
        return Err("Session title must not be empty".into());
    }
    let _guard = SESSION_IO_LOCK.lock().await;
    let mut data = load_session_unlocked(session_id).await?;
    data.session.title = bounded_title(title);
    save_session_unlocked(data).await?;
    load_session_unlocked(session_id).await
}

pub async fn toggle_pin_session(session_id: &str) -> Result<Session, String> {
    let _guard = SESSION_IO_LOCK.lock().await;
    let mut data = load_session_unlocked(session_id).await?;
    data.session.pinned = !data.session.pinned;
    if data.session.pinned {
        data.session.pinned_at = Some(chrono::Utc::now().timestamp_millis());
    } else {
        data.session.pinned_at = None;
    }
    let updated_session = data.session.clone();
    save_session_unlocked_with_pin_policy(data, true).await?;
    Ok(updated_session)
}

pub async fn delete_session(session_id: &str) -> Result<(), String> {
    let _guard = SESSION_IO_LOCK.lock().await;
    let path = session_path(session_id)?;
    if path.exists() {
        fs::remove_file(path)
            .await
            .map_err(|error| format!("Could not delete session: {error}"))?;
    }
    let _ = super::search::delete_session_index(session_id);
    Ok(())
}

fn bounded_summary_text(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text: String = normalized.chars().take(max_chars).collect();
    if normalized.chars().count() > max_chars {
        text.push('…');
    }
    text
}

fn tool_call_path(call: &super::types::ToolCallItem) -> Option<String> {
    let from_value = |value: &serde_json::Value| {
        value
            .get("path")
            .or_else(|| value.get("filePath"))
            .or_else(|| value.get("file_path"))
            .and_then(|path| path.as_str())
            .map(str::to_string)
    };
    match &call.args {
        serde_json::Value::String(raw) => serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|value| from_value(&value)),
        value => from_value(value),
    }
}

fn deterministic_summary(messages: &[ChatMessage]) -> String {
    let mut goals = Vec::new();
    let mut assistant_conclusions = Vec::new();
    let mut tool_activity = Vec::new();
    let mut files_touched = std::collections::BTreeSet::new();

    for msg in messages {
        if msg.role == "user" && !msg.content.trim().is_empty() {
            let line = msg.content.lines().next().unwrap_or("").trim();
            if !line.is_empty() {
                goals.push(bounded_summary_text(line, 400));
            }
        }
        if msg.role == "assistant" && !msg.content.trim().is_empty() {
            assistant_conclusions.push(bounded_summary_text(&msg.content, 1_000));
        }
        if let Some(calls) = &msg.tool_calls {
            for call in calls {
                if let Some(path) = tool_call_path(call) {
                    files_touched.insert(path.clone());
                    tool_activity.push(format!("{}: {} ({})", call.name, call.status, path));
                } else {
                    tool_activity.push(format!("{}: {}", call.name, call.status));
                }
                if let Some(files) = &call.files_changed {
                    for file in files {
                        files_touched.insert(file.path.clone());
                    }
                }
            }
        }
    }

    let mut out = String::new();
    if !goals.is_empty() {
        out.push_str("#### User Goals & Requirements\n");
        let start = goals.len().saturating_sub(8);
        for goal in &goals[start..] {
            out.push_str(&format!("- {}\n", goal));
        }
    }
    if !assistant_conclusions.is_empty() {
        out.push_str("\n#### Recent Technical Conclusions\n");
        let start = assistant_conclusions.len().saturating_sub(3);
        for conclusion in &assistant_conclusions[start..] {
            out.push_str(&format!("- {}\n", conclusion));
        }
    }
    if !files_touched.is_empty() {
        out.push_str("\n#### Files Touched or Referenced\n");
        for file in files_touched.iter().take(24) {
            out.push_str(&format!("- `{}`\n", file));
        }
    }
    if !tool_activity.is_empty() {
        out.push_str("\n#### Recent Tool Activity\n");
        let start = tool_activity.len().saturating_sub(12);
        for activity in &tool_activity[start..] {
            out.push_str(&format!("- {}\n", activity));
        }
    }
    if out.is_empty() {
        out = "Prior session context was consolidated into a local checkpoint.".into();
    }
    out
}

fn compact_data(mut data: SessionData) -> Result<SessionData, String> {
    if data.messages.len() < 3 {
        return Err("Conversation is too short to compact (requires at least 3 messages).".into());
    }
    if data
        .messages
        .iter()
        .any(|message| message.is_streaming == Some(true))
    {
        return Err("Stop the active run before compacting this conversation.".into());
    }

    // Preserve the complete latest user turn, including its assistant response and tools.
    let keep_start = data
        .messages
        .iter()
        .rposition(|message| message.role == "user")
        .ok_or("Conversation has no user turn to preserve")?;
    let has_prior_user_turn = data.messages[..keep_start]
        .iter()
        .any(|message| message.role == "user");
    if !has_prior_user_turn {
        return Err("At least two user turns are required before compaction is useful.".into());
    }

    let messages_to_summarize = data.messages[..keep_start].to_vec();
    let recent_messages = data.messages[keep_start..].to_vec();
    let summary = deterministic_summary(&messages_to_summarize);

    let checkpoint_message = ChatMessage {
        id: format!("compact_{}", Uuid::new_v4().simple()),
        role: "system".into(),
        content: format!(
            "## Compacted Context Checkpoint\n\n*Compacted {} previous messages using local deterministic history.*\n\n{}",
            messages_to_summarize.len(),
            summary
        ),
        timestamp: chrono::Utc::now().timestamp_millis(),
        reasoning: None,
        tool_calls: None,
        is_streaming: Some(false),
        attachments: None,
        council_turn: None,
        run_id: None,
        run_status: None,
        self_improvement: None,
    };

    data.messages = std::iter::once(checkpoint_message)
        .chain(recent_messages)
        .collect();
    Ok(data)
}

pub async fn compact_session(session_id: &str) -> Result<SessionData, String> {
    let _guard = SESSION_IO_LOCK.lock().await;
    let data = load_session_unlocked(session_id).await?;
    let compacted = compact_data(data)?;
    save_session_unlocked(compacted).await?;
    load_session_unlocked(session_id).await
}

pub async fn export_session(
    session_id: &str,
    destination: &str,
    format: &str,
) -> Result<(), String> {
    let _guard = SESSION_IO_LOCK.lock().await;
    let data = load_session_unlocked(session_id).await?;
    if destination.trim().is_empty() || destination.contains('\0') {
        return Err("An export destination is required".into());
    }
    let bytes = match format {
        "json" => serde_json::to_vec_pretty(&data).map_err(|error| error.to_string())?,
        "markdown" | "md" => render_markdown(&data).into_bytes(),
        "html" => render_standalone_html(&data).into_bytes(),
        _ => return Err("Export format must be 'json', 'markdown', or 'html'".into()),
    };
    atomic_write(Path::new(destination), &bytes).await
}

fn render_markdown(data: &SessionData) -> String {
    let mut output = format!(
        "# {}\n\nExported: {}\n\n",
        data.session.title,
        chrono::Utc::now().to_rfc3339()
    );
    for message in &data.messages {
        output.push_str(&format!("## {}\n\n{}\n\n", message.role, message.content));
        if let Some(reasoning) = &message.reasoning {
            output.push_str(&format!(
                "<details><summary>Reasoning</summary>\n\n{}\n\n</details>\n\n",
                reasoning
            ));
        }
    }
    if !data.artifacts.is_empty() {
        output.push_str("# Artifacts\n\n");
        for artifact in &data.artifacts {
            output.push_str(&format!(
                "## {}\n\n```{}\n{}\n```\n\n",
                artifact.title,
                artifact.language.clone().unwrap_or_default(),
                artifact.content
            ));
        }
    }
    output
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_standalone_html(data: &SessionData) -> String {
    let title = html_escape(&data.session.title);
    let date = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();

    let mut body = String::new();
    for msg in &data.messages {
        let role_class = if msg.role == "user" {
            "user"
        } else {
            "assistant"
        };
        let role_label = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        let content_html = html_escape(&msg.content);

        let reasoning_html = if let Some(r) = &msg.reasoning {
            if !r.trim().is_empty() {
                format!(
                    "<details class=\"reasoning\"><summary>Reasoning trace</summary><pre>{}</pre></details>",
                    html_escape(r)
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        body.push_str(&format!(
            r#"<div class="message {role_class}">
  <span class="badge {role_class}">{role_label}</span>
  {reasoning_html}
  <div class="content">{content_html}</div>
</div>
"#
        ));
    }

    let mut artifacts_html = String::new();
    if !data.artifacts.is_empty() {
        artifacts_html.push_str(r#"<div class="artifacts-section"><h2>Session Artifacts</h2>"#);
        for art in &data.artifacts {
            let art_title = html_escape(&art.title);
            let art_lang = html_escape(&art.language.clone().unwrap_or_else(|| "text".to_string()));
            let art_content = html_escape(&art.content);
            artifacts_html.push_str(&format!(
                r#"<div class="artifact-card">
  <h3>{art_title} <span class="lang">({art_lang})</span></h3>
  <pre><code>{art_content}</code></pre>
</div>
"#
            ));
        }
        artifacts_html.push_str("</div>");
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - AetherAgent Export</title>
<style>
  body {{
    background-color: #0d1117;
    color: #e6edf3;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
    line-height: 1.6;
    margin: 0;
    padding: 32px 16px;
  }}
  .container {{ max-width: 840px; margin: 0 auto; }}
  h1 {{ font-size: 24px; color: #f0f6fc; margin-bottom: 4px; }}
  .meta {{ color: #8b949e; font-size: 12px; margin-bottom: 24px; border-bottom: 1px solid #30363d; padding-bottom: 12px; }}
  .message {{
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 12px;
    padding: 16px 20px;
    margin-bottom: 18px;
    white-space: pre-wrap;
    word-break: break-word;
  }}
  .message.user {{
    background: #161f2e;
    border-color: #388bfd50;
  }}
  .badge {{
    display: inline-block;
    padding: 2px 8px;
    border-radius: 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    margin-bottom: 10px;
    letter-spacing: 0.05em;
  }}
  .badge.user {{ background: #1f6feb30; color: #58a6ff; }}
  .badge.assistant {{ background: #23863630; color: #3fb950; }}
  pre {{
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 12px;
    overflow-x: auto;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 1.45;
  }}
  code {{ font-family: inherit; }}
  details.reasoning {{
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 8px 12px;
    margin-bottom: 12px;
    font-size: 12px;
    color: #8b949e;
  }}
  details.reasoning summary {{ cursor: pointer; color: #f0883e; font-weight: 600; outline: none; }}
  .artifacts-section {{ margin-top: 40px; border-top: 2px solid #30363d; padding-top: 20px; }}
  .artifact-card {{ margin-bottom: 24px; }}
  .artifact-card h3 {{ font-size: 15px; color: #58a6ff; margin-bottom: 8px; }}
  .artifact-card .lang {{ font-size: 11px; color: #8b949e; font-weight: normal; }}
  @media print {{
    body {{ background: #ffffff !important; color: #111111 !important; padding: 0 !important; }}
    .message {{ background: #ffffff !important; border: 1px solid #dddddd !important; page-break-inside: avoid; }}
    .message.user {{ border-color: #999999 !important; }}
    pre {{ background: #f6f8fa !important; border: 1px solid #e1e4e8 !important; color: #24292e !important; }}
    .badge.user {{ background: #eeeeee !important; color: #000000 !important; }}
    .badge.assistant {{ background: #eeeeee !important; color: #000000 !important; }}
  }}
</style>
</head>
<body>
<div class="container">
  <h1>{title}</h1>
  <div class="meta">Exported from AetherAgent on {date} &bull; Print to PDF ready (Ctrl+P)</div>
  {body}
  {artifacts_html}
</div>
</body>
</html>"#
    )
}

pub(crate) async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Target has no parent directory")?;
    if parent.as_os_str().is_empty() {
        return Err("Export destination must include a parent directory".into());
    }
    fs::create_dir_all(parent)
        .await
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let temp = parent.join(format!(".aether-{}.tmp", Uuid::new_v4()));
    let write_result = async {
        let mut file = fs::File::create(&temp).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        Ok::<(), std::io::Error>(())
    }
    .await;
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp).await;
        return Err(format!("Could not write temporary data: {error}"));
    }
    if let Err(error) = fs::rename(&temp, path).await {
        let _ = fs::remove_file(&temp).await;
        return Err(format!("Could not replace {}: {error}", path.display()));
    }
    Ok(())
}

pub fn load_workspace_sync() -> String {
    if let Ok(content) = std::fs::read_to_string(workspace_path()) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(cwd) = value.get("cwd").and_then(|value| value.as_str()) {
                if Path::new(cwd).is_dir() {
                    return cwd.to_string();
                }
            }
        }
    }
    String::new()
}

pub async fn clear_workspace() -> Result<(), String> {
    let path = workspace_path();
    if path.exists() {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

pub async fn save_workspace(cwd: &str) -> Result<(), String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return clear_workspace().await;
    }
    let canonical = std::fs::canonicalize(trimmed)
        .map_err(|error| format!("Workspace is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err("Workspace must be an existing directory".into());
    }
    let content = serde_json::to_vec_pretty(&serde_json::json!({"cwd":canonical}))
        .map_err(|error| error.to_string())?;
    atomic_write(&workspace_path(), &content).await
}

pub fn validate_mcp_server(server: &McpServerConfig) -> Result<(), String> {
    validate_safe_id(&server.id)?;
    if server.enabled && server.command.trim().is_empty() {
        return Err("Enabled MCP servers require a command".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_survives_stale_session_saves_without_resurrecting_deleted_turns() {
        let mut stored = data_with_messages(vec![message("assistant", "Fixed")]);
        stored.messages[0].run_id = Some("run-1".into());
        stored.messages[0].self_improvement = Some(super::super::types::SelfImprovementEvent {
            run_id: "run-1".into(),
            session_id: "test-session".into(),
            action: super::super::types::SelfImprovementAction::SkillCreated,
            skill_name: Some("test-recipe".into()),
            summary: None,
            workspace_cwd: None,
        });
        let mut stale = stored.clone();
        stale.messages[0].self_improvement = None;
        merge_self_improvements(&mut stale, &stored);
        assert_eq!(
            stale.messages[0].self_improvement,
            stored.messages[0].self_improvement
        );
        stale.messages.clear();
        merge_self_improvements(&mut stale, &stored);
        assert!(stale.messages.is_empty());
    }

    fn message(role: &str, content: &str) -> ChatMessage {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "role": role,
            "content": content,
            "timestamp": 1
        }))
        .unwrap()
    }

    fn data_with_messages(messages: Vec<ChatMessage>) -> SessionData {
        SessionData {
            session: Session {
                id: "test-session".into(),
                title: "Test".into(),
                created_at: 1,
                updated_at: 1,
                message_count: messages.len(),
                workspace_cwd: None,
                preview: None,
                pinned: false,
                pinned_at: None,
            },
            messages,
            artifacts: Vec::new(),
            model_override: None,
        }
    }

    #[test]
    fn unpinning_a_session_is_not_resurrected_by_stale_disk_state() {
        let mut stored = data_with_messages(vec![message("user", "hi")]);
        stored.session.pinned = true;
        stored.session.pinned_at = Some(10);

        // Save biasa dari payload tanpa info pin: pin dilindungi dari state default.
        let mut default_payload = data_with_messages(vec![message("user", "hi")]);
        merge_pin_state(&mut default_payload.session, &stored.session, false);
        assert!(
            default_payload.session.pinned,
            "save biasa harus mempertahankan pin yang ada di disk"
        );

        // Payload hasil toggle_pin_session (pin dilepas eksplisit): harus menang.
        let mut unpinned = data_with_messages(vec![message("user", "hi")]);
        unpinned.session.pinned = false;
        unpinned.session.pinned_at = None;
        merge_pin_state(&mut unpinned.session, &stored.session, true);
        assert!(
            !unpinned.session.pinned,
            "pin yang sudah dilepas tidak boleh dihidupkan kembali oleh save"
        );
        assert_eq!(unpinned.session.pinned_at, None);
    }

    #[test]
    fn migrates_legacy_session_shape() {
        let value = serde_json::json!({"id":"sess-1","title":"Legacy","createdAt":1,"updatedAt":2,"messages":[],"artifacts":[]});
        let data = parse_session(value).unwrap();
        assert_eq!(data.session.id, "sess-1");
        assert_eq!(data.session.title, "Legacy");
    }

    #[test]
    fn rejects_path_shaped_session_ids() {
        assert!(session_path("../escape").is_err());
        assert!(session_path("valid-session_1").is_ok());
    }

    #[test]
    fn creates_compact_preview() {
        let data = vec![ChatMessage {
            id: "1".into(),
            role: "user".into(),
            content: "  hello\n world ".into(),
            timestamp: 1,
            reasoning: None,
            tool_calls: None,
            is_streaming: None,
            attachments: None,
            council_turn: None,
            run_id: None,
            run_status: None,
            self_improvement: None,
        }];
        assert_eq!(session_preview(&data).as_deref(), Some("hello world"));
    }

    #[test]
    fn compaction_is_local_turn_aware_and_reads_object_arguments() {
        let mut first_answer = message("assistant", "Implemented the parser");
        first_answer.tool_calls = Some(vec![super::super::types::ToolCallItem {
            id: "call-1".into(),
            name: "edit_file".into(),
            args: serde_json::json!({"path": "src/main.rs"}),
            status: "done".into(),
            ..Default::default()
        }]);
        let data = data_with_messages(vec![
            message("user", "Implement a parser"),
            first_answer,
            message("user", "Add focused tests"),
            message("assistant", "Tests added"),
        ]);
        let compacted = compact_data(data).unwrap();
        assert_eq!(compacted.messages.len(), 3);
        assert_eq!(compacted.messages[0].role, "system");
        assert!(compacted.messages[0].content.contains("src/main.rs"));
        assert!(compacted.messages[0]
            .content
            .contains("Implemented the parser"));
        assert_eq!(compacted.messages[1].content, "Add focused tests");
        assert_eq!(compacted.messages[2].content, "Tests added");
    }

    #[test]
    fn compaction_rejects_in_progress_or_single_turn_histories() {
        let mut streaming = data_with_messages(vec![
            message("user", "one"),
            message("assistant", "answer"),
            message("user", "two"),
            message("assistant", "pending"),
        ]);
        streaming.messages[3].is_streaming = Some(true);
        assert!(compact_data(streaming).is_err());

        let single_turn =
            data_with_messages(vec![message("user", "one"), message("assistant", "answer")]);
        assert!(compact_data(single_turn).is_err());
    }

    #[test]
    fn session_deserialization_defaults_pinned_to_false() {
        let raw = serde_json::json!({
            "id": "sess-legacy",
            "title": "Legacy Session",
            "createdAt": 1000,
            "updatedAt": 2000,
            "messageCount": 0
        });
        let session: Session = serde_json::from_value(raw).unwrap();
        assert!(!session.pinned);
        assert_eq!(session.pinned_at, None);
    }

    #[test]
    fn pinned_sessions_sort_ahead_of_recent_unpinned_sessions() {
        let s1 = Session {
            id: "s1".into(),
            title: "Old Pinned".into(),
            created_at: 100,
            updated_at: 100,
            message_count: 0,
            workspace_cwd: None,
            preview: None,
            pinned: true,
            pinned_at: Some(500),
        };
        let s2 = Session {
            id: "s2".into(),
            title: "Newer Unpinned".into(),
            created_at: 1000,
            updated_at: 2000,
            message_count: 0,
            workspace_cwd: None,
            preview: None,
            pinned: false,
            pinned_at: None,
        };
        let s3 = Session {
            id: "s3".into(),
            title: "Newly Pinned".into(),
            created_at: 100,
            updated_at: 100,
            message_count: 0,
            workspace_cwd: None,
            preview: None,
            pinned: true,
            pinned_at: Some(900),
        };

        let mut list = [s1, s2, s3];
        list.sort_by(|a, b| match (a.pinned, b.pinned) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (true, true) => {
                let a_pin = a.pinned_at.unwrap_or(a.updated_at);
                let b_pin = b.pinned_at.unwrap_or(b.updated_at);
                b_pin.cmp(&a_pin)
            }
            (false, false) => b.updated_at.cmp(&a.updated_at),
        });

        assert_eq!(list[0].id, "s3");
        assert_eq!(list[1].id, "s1");
        assert_eq!(list[2].id, "s2");
    }

    #[test]
    fn global_artifact_item_round_trips_json() {
        let item = GlobalArtifactItem {
            session_id: "sess-1".into(),
            session_title: "Coding Session".into(),
            session_updated_at: 12345678,
            artifact: super::super::types::Artifact {
                id: "art-1".into(),
                title: "index.ts".into(),
                artifact_type: "code".into(),
                language: Some("typescript".into()),
                content: "console.log('hi');".into(),
                file_path: Some("src/index.ts".into()),
                timestamp: 12345600,
            },
        };
        let val = serde_json::to_value(&item).unwrap();
        assert_eq!(val["sessionId"], "sess-1");
        assert_eq!(val["sessionTitle"], "Coding Session");
        assert_eq!(val["sessionUpdatedAt"], 12345678);
        assert_eq!(val["artifact"]["title"], "index.ts");
        assert_eq!(val["artifact"]["type"], "code");
    }
}
