use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Openai,
    Anthropic,
    Deepseek,
    Ollama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Minimal,
    Low,
    #[default]
    Medium,
    High,
    ExtraHigh,
    Max,
    Ultra,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::ExtraHigh => "extra_high",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }

    pub fn anthropic_budget(self, max_tokens: u32) -> u32 {
        let requested = match self {
            Self::Minimal => 1_024,
            Self::Low => 2_048,
            Self::Medium => 4_096,
            Self::High => 8_192,
            Self::ExtraHigh => 16_384,
            Self::Max => 32_768,
            Self::Ultra => 65_536,
        };
        requested.min(max_tokens.saturating_sub(1_024).max(1_024))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub model: String,
    pub models: Vec<String>,
    #[serde(rename = "customModels")]
    pub custom_models: Vec<String>,
    #[serde(rename = "extraHeaders")]
    pub extra_headers: HashMap<String, String>,
    #[serde(rename = "extraBody")]
    pub extra_body: serde_json::Map<String, serde_json::Value>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: ProviderKind::Openai,
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            models: Vec::new(),
            custom_models: Vec::new(),
            extra_headers: HashMap::new(),
            extra_body: serde_json::Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_compaction_threshold() -> u32 {
    65_536
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub providers: Vec<ProviderConfig>,
    #[serde(rename = "activeProviderId")]
    pub active_provider_id: String,
    #[serde(rename = "systemPrompt")]
    pub system_prompt: String,
    pub temperature: f64,
    #[serde(rename = "streamResponse")]
    pub stream_response: bool,
    #[serde(rename = "maxTokens")]
    pub max_tokens: Option<u32>,
    pub thinking: bool,
    #[serde(rename = "reasoningEffort")]
    pub reasoning_effort: ReasoningEffort,
    #[serde(rename = "favoriteModels")]
    pub favorite_models: Vec<String>,
    #[serde(rename = "skillPaths")]
    pub skill_paths: Vec<String>,
    #[serde(rename = "mcpServers")]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(rename = "maxTurns")]
    pub max_turns: usize,
    #[serde(rename = "notificationsEnabled", default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(rename = "soundChimeEnabled")]
    pub sound_chime_enabled: bool,
    pub self_improvement_enabled: bool,
    #[serde(rename = "autoCompactionEnabled", default = "default_true")]
    pub auto_compaction_enabled: bool,
    #[serde(
        rename = "autoCompactionThresholdTokens",
        default = "default_compaction_threshold"
    )]
    pub auto_compaction_threshold_tokens: u32,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            active_provider_id: String::new(),
            system_prompt: String::new(),
            temperature: 0.7,
            stream_response: true,
            max_tokens: Some(8_192),
            thinking: false,
            reasoning_effort: ReasoningEffort::Medium,
            favorite_models: Vec::new(),
            skill_paths: Vec::new(),
            mcp_servers: Vec::new(),
            max_turns: 24,
            notifications_enabled: true,
            sound_chime_enabled: false,
            self_improvement_enabled: false,
            auto_compaction_enabled: true,
            auto_compaction_threshold_tokens: 65_536,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiffStat {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ToolCallItem {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
    pub status: String,
    pub result: Option<String>,
    #[serde(rename = "artifactId")]
    pub artifact_id: Option<String>,
    #[serde(rename = "argumentsRaw")]
    pub arguments_raw: Option<String>,
    #[serde(rename = "startedAt")]
    pub started_at: Option<i64>,
    #[serde(rename = "completedAt")]
    pub completed_at: Option<i64>,
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<u64>,
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
    #[serde(rename = "terminalOutput")]
    pub terminal_output: Option<String>,
    #[serde(rename = "filesChanged")]
    pub files_changed: Option<Vec<FileDiffStat>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachedContext {
    pub id: String,
    pub path: String,
    pub name: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CouncilCandidate {
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub model: String,
    pub content: String,
    pub reasoning: Option<String>,
    #[serde(rename = "isStreaming")]
    pub is_streaming: Option<bool>,
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<i64>,
    pub winner: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilTurn {
    #[serde(rename = "candidateA")]
    pub candidate_a: CouncilCandidate,
    #[serde(rename = "candidateB")]
    pub candidate_b: CouncilCandidate,
    #[serde(rename = "chosenWinner", default)]
    pub chosen_winner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfImprovementAction {
    SkillCreated,
    SkillUpdated,
    NoAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfImprovementEvent {
    pub run_id: String,
    pub session_id: String,
    pub action: SelfImprovementAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(rename = "toolCalls", default)]
    pub tool_calls: Option<Vec<ToolCallItem>>,
    #[serde(rename = "isStreaming", default)]
    pub is_streaming: Option<bool>,
    #[serde(default)]
    pub attachments: Option<Vec<AttachedContext>>,
    #[serde(rename = "councilTurn", default)]
    pub council_turn: Option<CouncilTurn>,
    #[serde(rename = "runId", default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(rename = "runStatus", default, skip_serializing_if = "Option::is_none")]
    pub run_status: Option<String>,
    #[serde(
        rename = "selfImprovement",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub self_improvement: Option<SelfImprovementEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preamble: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub artifact_type: String,
    #[serde(default)]
    pub language: Option<String>,
    pub content: String,
    #[serde(rename = "filePath", default)]
    pub file_path: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalArtifactItem {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "sessionTitle")]
    pub session_title: String,
    #[serde(rename = "sessionUpdatedAt")]
    pub session_updated_at: i64,
    pub artifact: Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(rename = "messageCount")]
    pub message_count: usize,
    #[serde(rename = "workspaceCwd", default)]
    pub workspace_cwd: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(rename = "pinnedAt", default)]
    pub pinned_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionModelOverride {
    #[serde(rename = "providerId")]
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<bool>,
    #[serde(rename = "reasoningEffort")]
    pub reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub session: Session,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(rename = "modelOverride", default)]
    pub model_override: Option<SessionModelOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRequest {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub provider: ProviderConfig,
    #[serde(rename = "systemPrompt", default)]
    pub system_prompt: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub thinking: bool,
    #[serde(rename = "reasoningEffort", default)]
    pub reasoning_effort: ReasoningEffort,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(rename = "maxTokens", default)]
    pub max_tokens: Option<u32>,
    #[serde(rename = "maxTurns", default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(rename = "streamResponse", default = "default_stream")]
    pub stream_response: bool,
    #[serde(rename = "skillPaths", default)]
    pub skill_paths: Vec<String>,
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub self_improvement_enabled: bool,
    #[serde(rename = "autoCompactionEnabled", default = "default_true")]
    pub auto_compaction_enabled: bool,
    #[serde(rename = "autoCompactionThresholdTokens", default)]
    pub auto_compaction_threshold_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroCompactionEvent {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "prunedToolCount")]
    pub pruned_tool_count: usize,
    #[serde(rename = "estimatedTokensSaved")]
    pub estimated_tokens_saved: usize,
}

fn default_temperature() -> f64 {
    0.7
}
fn default_max_turns() -> usize {
    24
}
fn default_stream() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentProgress {
    pub id: String,
    pub role: String,
    pub task: String,
    pub status: String,
    #[serde(rename = "startedAt")]
    pub started_at: i64,
    #[serde(rename = "completedAt", default)]
    pub completed_at: Option<i64>,
    #[serde(rename = "summary", default)]
    pub summary: Option<String>,
    #[serde(rename = "artifactId", default)]
    pub artifact_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubagentEventData {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub subagent: SubagentProgress,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillActivatedEventData {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub skill: SkillInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamEventData {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolEventData {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub tool: ToolCallItem,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactEventData {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub artifact: Artifact,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalEventData {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    pub delta: String,
    pub stream: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoneEventData {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub cancelled: bool,
    pub successful: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEventData {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouncilChunkData {
    pub run_id: String,
    pub session_id: String,
    pub candidate: String,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouncilDoneData {
    pub run_id: String,
    pub session_id: String,
    pub candidate: String,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    #[serde(rename = "chunk")]
    Chunk(StreamEventData),
    #[serde(rename = "reasoning")]
    Reasoning(StreamEventData),
    #[serde(rename = "tool_update")]
    ToolUpdate(ToolEventData),
    #[serde(rename = "artifact")]
    Artifact(ArtifactEventData),
    #[serde(rename = "terminal_output")]
    TerminalOutput(TerminalEventData),
    #[serde(rename = "done")]
    Done(DoneEventData),
    #[serde(rename = "error")]
    Error(ErrorEventData),
    #[serde(rename = "subagent_update")]
    SubagentUpdate(SubagentEventData),
    #[serde(rename = "skill_activated")]
    SkillActivated(SkillActivatedEventData),
    #[serde(rename = "council_chunk")]
    CouncilChunk(CouncilChunkData),
    #[serde(rename = "council_done")]
    CouncilDone(CouncilDoneData),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_is_opt_in_and_round_trips_in_settings() {
        let defaults: AgentSettings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(
            serde_json::to_value(defaults).unwrap()["self_improvement_enabled"],
            false
        );
        let enabled: AgentSettings = serde_json::from_value(serde_json::json!({
            "self_improvement_enabled": true
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(enabled).unwrap()["self_improvement_enabled"],
            true
        );
    }

    #[test]
    fn completed_message_preserves_reflection_and_run_identity() {
        let value = serde_json::json!({
            "id": "assistant-1", "role": "assistant", "content": "Fixed", "timestamp": 1,
            "runId": "run-1", "runStatus": "completed",
            "selfImprovement": {
                "run_id": "run-1", "session_id": "session-1", "action": "skill_created",
                "skill_name": "test-recipe", "summary": "Repeatable test recipe"
            }
        });
        let message: ChatMessage = serde_json::from_value(value.clone()).unwrap();
        let stored = serde_json::to_value(message).unwrap();
        for key in ["runId", "runStatus", "selfImprovement"] {
            assert_eq!(stored[key], value[key]);
        }
    }

    #[test]
    fn old_provider_shape_gets_forward_compatible_defaults() {
        let parsed: ProviderConfig = serde_json::from_str(
            r#"{"id":"x","name":"X","baseUrl":"https://example.test/v1","apiKey":"","model":"m"}"#,
        )
        .expect("legacy provider should deserialize");
        assert_eq!(parsed.kind, ProviderKind::Openai);
        assert!(parsed.models.is_empty());
    }

    #[test]
    fn event_contract_uses_camel_case_and_scopes() {
        let event = AgentEvent::Done(DoneEventData {
            run_id: "run-1".into(),
            session_id: "session-1".into(),
            cancelled: false,
            successful: true,
        });
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "done");
        assert_eq!(value["data"]["runId"], "run-1");
        assert_eq!(value["data"]["sessionId"], "session-1");
    }

    #[test]
    fn council_chunk_event_serializes_properly() {
        let event = AgentEvent::CouncilChunk(CouncilChunkData {
            run_id: "run-council-1".into(),
            session_id: "sess-1".into(),
            candidate: "A".into(),
            kind: "content".into(),
            text: "Hello from candidate A".into(),
        });
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "council_chunk");
        assert_eq!(value["data"]["candidate"], "A");
        assert_eq!(value["data"]["kind"], "content");
        assert_eq!(value["data"]["text"], "Hello from candidate A");
    }

    #[test]
    fn message_round_trip_preserves_attachments_and_council_state() {
        let value = serde_json::json!({
            "id": "message-1",
            "role": "assistant",
            "content": "winner",
            "timestamp": 1,
            "attachments": [{"id":"att-1","path":"D:/project/a.ts","name":"a.ts","isDir":false}],
            "councilTurn": {
                "candidateA": {"providerId":"a","model":"model-a","content":"A"},
                "candidateB": {"providerId":"b","model":"model-b","content":"B"},
                "chosenWinner": "B"
            }
        });
        let message: ChatMessage = serde_json::from_value(value).unwrap();
        let serialized = serde_json::to_value(message).unwrap();
        assert_eq!(serialized["attachments"][0]["name"], "a.ts");
        assert_eq!(serialized["councilTurn"]["chosenWinner"], "B");
        assert_eq!(serialized["councilTurn"]["candidateB"]["content"], "B");
    }
}
