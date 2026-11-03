use std::sync::Arc;
use std::sync::Mutex;
use tauri::{AppHandle, State};

mod agent;
use agent::runner::AgentRunner;
use agent::storage;
use agent::types::{AgentSettings, Artifact, ChatMessage, ProviderConfig, Session};

pub struct AppState {
    pub workspace_cwd: Mutex<String>,
    pub runner: Arc<AgentRunner>,
}

#[tauri::command]
async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: ProviderConfig,
    system_prompt: Option<String>,
    messages: Vec<ChatMessage>,
) -> Result<(), String> {
    let cwd = state.workspace_cwd.lock().unwrap().clone();
    let runner = state.runner.clone();

    tokio::spawn(async move {
        runner.run(app, cwd, provider, system_prompt, messages).await;
    });

    Ok(())
}

#[tauri::command]
fn abort_agent(state: State<'_, AppState>) -> Result<(), String> {
    state.runner.abort();
    Ok(())
}

#[tauri::command]
async fn get_settings() -> Result<AgentSettings, String> {
    Ok(storage::load_settings().await)
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
async fn load_session(session_id: String) -> Result<(Vec<ChatMessage>, Vec<Artifact>), String> {
    Ok(storage::load_session(&session_id).await)
}

#[tauri::command]
async fn save_session(session_id: String, title: String, messages: Vec<ChatMessage>, artifacts: Vec<Artifact>) -> Result<(), String> {
    storage::save_session(&session_id, &title, &messages, &artifacts).await
}

#[tauri::command]
async fn delete_session(session_id: String) -> Result<(), String> {
    storage::delete_session(&session_id).await
}

#[tauri::command]
async fn fetch_provider_models(base_url: String, api_key: String) -> Result<Vec<String>, String> {
    agent::provider::fetch_models(&base_url, &api_key).await
}

#[tauri::command]
fn get_workspace_cwd(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.workspace_cwd.lock().unwrap().clone())
}

#[tauri::command]
fn set_workspace_cwd(state: State<'_, AppState>, new_cwd: String) -> Result<(), String> {
    *state.workspace_cwd.lock().unwrap() = new_cwd;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            workspace_cwd: Mutex::new(initial_cwd),
            runner: Arc::new(AgentRunner::new()),
        })
        .invoke_handler(tauri::generate_handler![
            send_message,
            abort_agent,
            get_settings,
            save_settings,
            list_sessions,
            load_session,
            save_session,
            delete_session,
            fetch_provider_models,
            get_workspace_cwd,
            set_workspace_cwd
        ])
        .run(tauri::generate_context!())
        .expect("error while running AetherAgent application");
}
