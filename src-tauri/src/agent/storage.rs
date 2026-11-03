use std::path::PathBuf;
use tokio::fs;
use super::types::{AgentSettings, Artifact, ChatMessage, ProviderConfig, Session};

fn get_config_dir() -> PathBuf {
    let mut dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push(".aether-agent");
    dir
}

fn get_sessions_dir() -> PathBuf {
    let mut dir = get_config_dir();
    dir.push("sessions");
    dir
}

pub fn get_default_settings() -> AgentSettings {
    AgentSettings {
        providers: vec![
            ProviderConfig {
                id: "gutsai-kimi".to_string(),
                name: "GutsAI (Kimi K1.5 / K2)".to_string(),
                base_url: "https://api.gutsai.id/v1".to_string(),
                api_key: "".to_string(),
                model: "kimi-k1.5-preview".to_string(),
            },
            ProviderConfig {
                id: "deepseek-direct".to_string(),
                name: "DeepSeek Official".to_string(),
                base_url: "https://api.deepseek.com/v1".to_string(),
                api_key: "".to_string(),
                model: "deepseek-chat".to_string(),
            },
            ProviderConfig {
                id: "ollama-local".to_string(),
                name: "Ollama (Local)".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                api_key: "ollama".to_string(),
                model: "llama3.2".to_string(),
            },
        ],
        active_provider_id: "gutsai-kimi".to_string(),
        system_prompt: "".to_string(),
        temperature: 0.7,
    }
}

pub async fn load_settings() -> AgentSettings {
    let config_path = get_config_dir().join("config.json");
    if let Ok(content) = fs::read_to_string(&config_path).await {
        if let Ok(settings) = serde_json::from_str::<AgentSettings>(&content) {
            return settings;
        }
    }
    let default = get_default_settings();
    let _ = save_settings(&default).await;
    default
}

pub async fn save_settings(settings: &AgentSettings) -> Result<(), String> {
    let dir = get_config_dir();
    fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let path = dir.join("config.json");
    let content = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, content).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn list_sessions() -> Vec<Session> {
    let sdir = get_sessions_dir();
    let mut sessions = Vec::new();
    if let Ok(mut entries) = fs::read_dir(&sdir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(content) = fs::read_to_string(&path).await {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        let id = val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let title = val.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
                        let created_at = val.get("createdAt").and_then(|v| v.as_i64()).unwrap_or(0);
                        let updated_at = val.get("updatedAt").and_then(|v| v.as_i64()).unwrap_or(0);
                        let msg_count = val.get("messages").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);

                        sessions.push(Session {
                            id,
                            title,
                            created_at,
                            updated_at,
                            message_count: msg_count,
                        });
                    }
                }
            }
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

pub async fn load_session(session_id: &str) -> (Vec<ChatMessage>, Vec<Artifact>) {
    let path = get_sessions_dir().join(format!("{}.json", session_id));
    if let Ok(content) = fs::read_to_string(path).await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            let msgs = val.get("messages")
                .and_then(|v| serde_json::from_value::<Vec<ChatMessage>>(v.clone()).ok())
                .unwrap_or_default();
            let arts = val.get("artifacts")
                .and_then(|v| serde_json::from_value::<Vec<Artifact>>(v.clone()).ok())
                .unwrap_or_default();
            return (msgs, arts);
        }
    }
    (Vec::new(), Vec::new())
}

pub async fn save_session(session_id: &str, title: &str, msgs: &[ChatMessage], arts: &[Artifact]) -> Result<(), String> {
    let sdir = get_sessions_dir();
    fs::create_dir_all(&sdir).await.map_err(|e| e.to_string())?;
    let path = sdir.join(format!("{}.json", session_id));

    let data = serde_json::json!({
        "id": session_id,
        "title": title,
        "updatedAt": chrono::Utc::now().timestamp_millis(),
        "createdAt": chrono::Utc::now().timestamp_millis(),
        "messages": msgs,
        "artifacts": arts,
    });

    let content = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    fs::write(path, content).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn delete_session(session_id: &str) -> Result<(), String> {
    let path = get_sessions_dir().join(format!("{}.json", session_id));
    if path.exists() {
        fs::remove_file(path).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}
