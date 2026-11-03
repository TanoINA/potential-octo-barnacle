use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallItem {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
    pub status: String, // "running", "done", "error"
    pub result: Option<String>,
    #[serde(rename = "artifactId")]
    pub artifact_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String, // "user", "assistant", "system", "tool"
    pub content: String,
    pub timestamp: i64,
    pub reasoning: Option<String>,
    #[serde(rename = "toolCalls")]
    pub tool_calls: Option<Vec<ToolCallItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub artifact_type: String, // "code", "diff", "terminal", "markdown"
    pub language: Option<String>,
    pub content: String,
    #[serde(rename = "filePath")]
    pub file_path: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub model: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    pub providers: Vec<ProviderConfig>,
    #[serde(rename = "activeProviderId")]
    pub active_provider_id: String,
    #[serde(rename = "systemPrompt")]
    pub system_prompt: String,
    pub temperature: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    #[serde(rename = "chunk")]
    Chunk(String),
    #[serde(rename = "reasoning")]
    Reasoning(String),
    #[serde(rename = "tool_update")]
    ToolUpdate(ToolCallItem),
    #[serde(rename = "artifact")]
    Artifact(Artifact),
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    Error(String),
}
