use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use super::types::ProviderConfig;

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ChatCompletionFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[allow(dead_code)]
pub struct StreamResult {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ChatCompletionToolCall>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelItem {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<i64>,
    pub owned_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelsListResponse {
    pub data: Vec<ModelItem>,
}

pub async fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let clean_base = base_url.trim_end_matches('/');
    let url = format!("{}/models", clean_base);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let mut req = client.get(&url);
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = req.send().await.map_err(|e| format!("Failed to reach {}: {}", url, e))?;

    let status = response.status();
    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        return Err(format!("Server returned HTTP {}: {}", status, err_text));
    }

    let body_text = response.text().await.map_err(|e| e.to_string())?;

    // Standard OpenAI format {"data": [{"id": "..."}, ...]}
    if let Ok(parsed) = serde_json::from_str::<ModelsListResponse>(&body_text) {
        let mut ids: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
        ids.sort();
        return Ok(ids);
    }

    // Fallback array format [{"id": "..."}, ...]
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&body_text) {
        let mut ids = Vec::new();
        for item in arr {
            if let Some(id_str) = item.get("id").and_then(|v| v.as_str()) {
                ids.push(id_str.to_string());
            } else if let Some(str_val) = item.as_str() {
                ids.push(str_val.to_string());
            }
        }
        if !ids.is_empty() {
            ids.sort();
            return Ok(ids);
        }
    }

    Err("Could not parse models from provider response".to_string())
}

pub async fn stream_chat_completion<F1, F2, F3>(
    provider: &ProviderConfig,
    messages: &serde_json::Value,
    tools: &serde_json::Value,
    mut on_chunk: F1,
    mut on_reasoning: F2,
    mut on_tool_delta: F3,
    abort_flag: Arc<AtomicBool>,
) -> Result<StreamResult, String>
where
    F1: FnMut(String),
    F2: FnMut(String),
    F3: FnMut(usize, Option<String>, Option<String>, Option<String>),
{
    let clean_base = provider.base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", clean_base);

    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if !provider.api_key.is_empty() {
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", provider.api_key)) {
            headers.insert(AUTHORIZATION, val);
        }
    }

    let mut body = serde_json::json!({
        "model": provider.model,
        "messages": messages,
        "stream": true,
    });

    if let Some(tool_arr) = tools.as_array() {
        if !tool_arr.is_empty() {
            body["tools"] = tools.clone();
            body["tool_choice"] = serde_json::json!("auto");
        }
    }

    let response = client
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Failed to send request to {}: {}", url, e))?;

    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_default();
        return Err(format!("Provider returned HTTP status: {}", err_text));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    let mut accumulated_content = String::new();
    let mut accumulated_reasoning = String::new();
    let mut tool_calls_map: HashMap<usize, (String, String, String)> = HashMap::new(); // idx -> (id, name, args)

    while let Some(chunk_res) = stream.next().await {
        if abort_flag.load(Ordering::Relaxed) {
            break;
        }

        let chunk = chunk_res.map_err(|e| format!("Stream error: {}", e))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);

            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            if line == "data: [DONE]" {
                break;
            }

            if let Some(json_str) = line.strip_prefix("data: ") {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let Some(delta) = parsed.pointer("/choices/0/delta") {
                        // 1. Extended reasoning (DeepSeek R1 / Kimi)
                        if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                            accumulated_reasoning.push_str(reasoning);
                            on_reasoning(reasoning.to_string());
                        }

                        // 2. Normal content
                        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                            accumulated_content.push_str(content);
                            on_chunk(content.to_string());
                        }

                        // 3. Tool calls
                        if let Some(tc_arr) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                            for tc in tc_arr {
                                let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                                let id_delta = tc.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                                let name_delta = tc.pointer("/function/name").and_then(|v| v.as_str()).map(|s| s.to_string());
                                let args_delta = tc.pointer("/function/arguments").and_then(|v| v.as_str()).map(|s| s.to_string());

                                let entry = tool_calls_map.entry(idx).or_insert((String::new(), String::new(), String::new()));
                                if let Some(ref id) = id_delta { entry.0 = id.clone(); }
                                if let Some(ref name) = name_delta { entry.1 = name.clone(); }
                                if let Some(ref args) = args_delta { entry.2.push_str(args); }

                                on_tool_delta(idx, id_delta, name_delta, args_delta);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut final_tool_calls = Vec::new();
    let mut indices: Vec<usize> = tool_calls_map.keys().copied().collect();
    indices.sort();

    for idx in indices {
        if let Some((id, name, args)) = tool_calls_map.remove(&idx) {
            final_tool_calls.push(ChatCompletionToolCall {
                id: if id.is_empty() { format!("call_{}", idx) } else { id },
                call_type: "function".to_string(),
                function: ChatCompletionFunctionCall {
                    name,
                    arguments: args,
                },
            });
        }
    }

    Ok(StreamResult {
        content: accumulated_content,
        reasoning: accumulated_reasoning,
        tool_calls: final_tool_calls,
    })
}
