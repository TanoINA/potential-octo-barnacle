use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use super::provider::stream_chat_completion;
use super::tools::{execute_tool, get_agent_tools};
use super::types::{AgentEvent, ChatMessage, ProviderConfig, ToolCallItem};

pub struct AgentRunner {
    pub abort_flag: Arc<AtomicBool>,
}

impl AgentRunner {
    pub fn new() -> Self {
        Self {
            abort_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn abort(&self) {
        self.abort_flag.store(true, Ordering::Relaxed);
    }

    pub async fn run(
        &self,
        app: AppHandle,
        workspace_cwd: String,
        provider: ProviderConfig,
        system_prompt: Option<String>,
        messages: Vec<ChatMessage>,
    ) {
        self.abort_flag.store(false, Ordering::Relaxed);

        let default_sys = "You are AetherAgent, an ultra-fast, expert coding assistant built with Rust.
You have native access to tools: read_file, write_file, list_directory, run_command (PowerShell).
Inspect files before editing. Output clear, concise answers.".to_string();

        let mut conversation: Vec<serde_json::Value> = Vec::new();
        conversation.push(serde_json::json!({
            "role": "system",
            "content": system_prompt.unwrap_or(default_sys)
        }));

        for msg in &messages {
            if msg.role == "user" || msg.role == "assistant" {
                conversation.push(serde_json::json!({
                    "role": msg.role,
                    "content": msg.content
                }));
            }
        }

        let tools = get_agent_tools();
        let mut turns = 0;
        const MAX_TURNS: usize = 12;

        while turns < MAX_TURNS {
            if self.abort_flag.load(Ordering::Relaxed) {
                break;
            }
            turns += 1;

            let app_chunk = app.clone();
            let app_reasoning = app.clone();

            let stream_res = stream_chat_completion(
                &provider,
                &serde_json::Value::Array(conversation.clone()),
                &tools,
                move |chunk| {
                    let _ = app_chunk.emit("agent-event", AgentEvent::Chunk(chunk));
                },
                move |reasoning| {
                    let _ = app_reasoning.emit("agent-event", AgentEvent::Reasoning(reasoning));
                },
                move |_idx, _id, _name, _args| {
                    // delta tool event
                },
                self.abort_flag.clone(),
            )
            .await;

            match stream_res {
                Ok(result) => {
                    if result.tool_calls.is_empty() {
                        // Finished response without needing tools
                        break;
                    }

                    // Model called tools
                    conversation.push(serde_json::json!({
                        "role": "assistant",
                        "content": if result.content.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(result.content) },
                        "tool_calls": serde_json::to_value(&result.tool_calls).unwrap_or(serde_json::Value::Null)
                    }));

                    for tc in result.tool_calls {
                        if self.abort_flag.load(Ordering::Relaxed) {
                            break;
                        }

                        let args_val: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or_else(|_| serde_json::json!({ "raw": tc.function.arguments }));

                        let mut tool_item = ToolCallItem {
                            id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            args: args_val.clone(),
                            status: "running".to_string(),
                            result: None,
                            artifact_id: None,
                        };
                        let _ = app.emit("agent-event", AgentEvent::ToolUpdate(tool_item.clone()));

                        let exec_res = execute_tool(&tc.function.name, &args_val, &workspace_cwd).await;

                        if let Some(artifact) = exec_res.artifact {
                            tool_item.artifact_id = Some(artifact.id.clone());
                            let _ = app.emit("agent-event", AgentEvent::Artifact(artifact));
                        }

                        tool_item.status = "done".to_string();
                        tool_item.result = Some(exec_res.output.clone());
                        let _ = app.emit("agent-event", AgentEvent::ToolUpdate(tool_item));

                        conversation.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tc.id,
                            "content": exec_res.output
                        }));
                    }
                }
                Err(err) => {
                    let _ = app.emit("agent-event", AgentEvent::Error(err));
                    break;
                }
            }
        }

        let _ = app.emit("agent-event", AgentEvent::Done);
    }
}
