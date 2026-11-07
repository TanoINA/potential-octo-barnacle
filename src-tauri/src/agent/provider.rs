use super::types::{ProviderConfig, ProviderKind, ReasoningEffort};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatCompletionToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ChatCompletionFunctionCall,
    /// Original provider stream/block index; never sent back to providers.
    #[serde(skip)]
    pub stream_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatCompletionFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Default)]
pub struct StreamResult {
    pub content: String,
    #[allow(dead_code)]
    pub reasoning: String,
    pub tool_calls: Vec<ChatCompletionToolCall>,
    pub cancelled: bool,
    /// Provider finish reason (`stop`, `length`, `tool_calls`, `end_turn`, ...).
    /// `None` when the provider did not report one.
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolDelta {
    pub index: usize,
    #[allow(dead_code)]
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CompletionOptions {
    pub thinking: bool,
    pub reasoning_effort: ReasoningEffort,
    pub temperature: f64,
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Default)]
struct ToolBuilder {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Default)]
struct StreamAccumulator {
    content: String,
    reasoning: String,
    tools: BTreeMap<usize, ToolBuilder>,
    cancelled: bool,
    finish_reason: Option<String>,
}

impl StreamAccumulator {
    fn finish(self) -> StreamResult {
        let tool_calls = self
            .tools
            .into_iter()
            .map(|(index, tool)| ChatCompletionToolCall {
                id: if tool.id.is_empty() {
                    format!("call_{index}")
                } else {
                    tool.id
                },
                call_type: "function".into(),
                function: ChatCompletionFunctionCall {
                    name: tool.name,
                    arguments: tool.arguments,
                },
                stream_index: index,
            })
            .collect();
        StreamResult {
            content: self.content,
            reasoning: self.reasoning,
            tool_calls,
            cancelled: self.cancelled,
            finish_reason: self.finish_reason,
        }
    }
}

/// Finish reasons that mean the model stopped on purpose rather than being cut
/// off by a token limit or a safety filter. OpenAI and Anthropic spell them
/// differently, so both spellings are accepted.
const COMPLETE_FINISH_REASONS: [&str; 6] = [
    "stop",
    "tool_calls",
    "function_call",
    "end_turn",
    "tool_use",
    "stop_sequence",
];

/// Rejects runs that only look successful: truncated by the token limit,
/// filtered, or empty. Returns an explicit error so the caller never reports a
/// silent success.
fn validate_stream_result(result: &StreamResult) -> Result<(), String> {
    if result.cancelled {
        return Ok(());
    }
    if let Some(reason) = result.finish_reason.as_deref() {
        if !COMPLETE_FINISH_REASONS.contains(&reason) {
            return Err(format!(
                "Provider stream ended with finish_reason '{reason}': the response was truncated or filtered before completion"
            ));
        }
    }
    if result.content.trim().is_empty()
        && result.reasoning.trim().is_empty()
        && result.tool_calls.is_empty()
    {
        return Err(
            "Provider stream produced no content, reasoning, or tool calls before completing"
                .into(),
        );
    }
    Ok(())
}

pub async fn fetch_models(provider: &ProviderConfig) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Failed to build HTTP client: {error}"))?;
    let primary_url = endpoint(&provider.base_url, "/v1/models", "/models");
    let primary = fetch_models_from(&client, provider, &primary_url).await;
    match primary {
        Ok(models) if !models.is_empty() => Ok(models),
        Ok(_) | Err(_) if provider.kind == ProviderKind::Ollama => {
            let base = provider.base_url.trim().trim_end_matches('/');
            let native_base = base.strip_suffix("/v1").unwrap_or(base);
            let native =
                fetch_models_from(&client, provider, &format!("{native_base}/api/tags")).await;
            match (native, primary) {
                (Ok(models), _) => Ok(models),
                (Err(native_error), Err(primary_error)) => Err(format!("OpenAI-compatible discovery failed: {primary_error}; Ollama tags failed: {native_error}")),
                (Err(error), _) => Err(error),
            }
        }
        Ok(models) => Ok(models),
        Err(error) => Err(error),
    }
}

async fn fetch_models_from(
    client: &reqwest::Client,
    provider: &ProviderConfig,
    url: &str,
) -> Result<Vec<String>, String> {
    let response = client
        .get(url)
        .headers(request_headers(provider)?)
        .send()
        .await
        .map_err(|error| format!("Failed to reach {url}: {error}"))?;
    let status = response.status();
    let body = read_bounded_response(response, &CancellationToken::new(), 2 * 1024 * 1024)
        .await
        .map_err(|error| format!("Could not read model response: {error}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", bounded_error(&body)));
    }
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("Provider returned invalid model JSON: {error}"))?;
    let mut models = Vec::new();
    if let Some(entries) = value.get("data").and_then(|v| v.as_array()) {
        collect_model_ids(entries, &mut models);
    }
    if let Some(entries) = value.get("models").and_then(|v| v.as_array()) {
        collect_model_ids(entries, &mut models);
        for entry in entries {
            if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
                models.push(name.to_string());
            }
        }
    }
    if let Some(entries) = value.as_array() {
        collect_model_ids(entries, &mut models);
    }
    models.retain(|model| !model.trim().is_empty());
    models.sort();
    models.dedup();
    if models.is_empty() && value.get("data").is_none() && value.get("models").is_none() {
        return Err("Could not find a model array in the provider response".into());
    }
    Ok(models)
}

fn collect_model_ids(entries: &[serde_json::Value], output: &mut Vec<String>) {
    for entry in entries {
        if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
            output.push(id.to_string());
        } else if let Some(id) = entry.as_str() {
            output.push(id.to_string());
        }
    }
}

pub const PROVIDER_STALE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_PROVIDER_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Classifies provider errors into retryable (network drops, 5xx server errors,
/// rate limits, stalled stream timeouts) vs non-retryable (bad request, auth failure, user cancel).
pub fn is_retryable_provider_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    if lower.contains("cancelled") || lower.contains("canceled") {
        return false;
    }
    if lower.contains("http 400")
        || lower.contains("http 401")
        || lower.contains("http 403")
        || lower.contains("http 404")
        || lower.contains("unauthorized")
        || lower.contains("invalid api key")
        || lower.contains("base url is required")
        || lower.contains("a model must be selected")
        || lower.contains("finish_reason")
    {
        return false;
    }
    lower.contains("http 429")
        || lower.contains("http 500")
        || lower.contains("http 502")
        || lower.contains("http 503")
        || lower.contains("http 504")
        || lower.contains("http 52")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("stalled")
        || lower.contains("connection")
        || lower.contains("network")
        || lower.contains("reset by peer")
        || lower.contains("broken pipe")
        || lower.contains("stream failed")
        || lower.contains("request failed")
        || lower.contains("produced no content, reasoning, or tool calls")
        || lower.contains("dns")
        || lower.contains("eof")
}

async fn read_bounded_response(
    response: reqwest::Response,
    cancel: &CancellationToken,
    max_bytes: usize,
) -> Result<String, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err("Provider response exceeds the size limit".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    loop {
        let item = tokio::select! {
            _ = cancel.cancelled() => return Err("Provider response cancelled".into()),
            result = tokio::time::timeout(PROVIDER_STALE_TIMEOUT, stream.next()) => {
                result.map_err(|_| "Provider response timed out after 60s waiting for data".to_string())?
            }
        };
        let Some(item) = item else { break };
        let chunk = item.map_err(|error| format!("Provider response stream failed: {error}"))?;
        if bytes.len() + chunk.len() > max_bytes {
            return Err("Provider response exceeded the size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| "Provider response was not valid UTF-8".into())
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_chat_completion<F1, F2, F3>(
    provider: &ProviderConfig,
    messages: &serde_json::Value,
    tools: &serde_json::Value,
    options: &CompletionOptions,
    mut on_chunk: F1,
    mut on_reasoning: F2,
    mut on_tool_delta: F3,
    cancel: CancellationToken,
) -> Result<StreamResult, String>
where
    F1: FnMut(String),
    F2: FnMut(String),
    F3: FnMut(ToolDelta),
{
    if provider.base_url.trim().is_empty() {
        return Err("Provider base URL is required".into());
    }
    if provider.model.trim().is_empty() {
        return Err("A model must be selected".into());
    }
    let (url, body) = match provider.kind {
        ProviderKind::Anthropic => (
            endpoint(&provider.base_url, "/v1/messages", "/messages"),
            anthropic_body(provider, messages, tools, options)?,
        ),
        _ => (
            endpoint(
                &provider.base_url,
                "/v1/chat/completions",
                "/chat/completions",
            ),
            openai_body(provider, messages, tools, options),
        ),
    };
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Failed to build HTTP client: {error}"))?;
    let response = tokio::select! {
        _ = cancel.cancelled() => return Ok(StreamResult { cancelled: true, ..StreamResult::default() }),
        result = tokio::time::timeout(
            PROVIDER_STALE_TIMEOUT,
            client.post(&url).headers(request_headers(provider)?).json(&body).send()
        ) => match result {
            Ok(Ok(resp)) => resp,
            Ok(Err(err)) => return Err(format!("Provider request failed: {err}")),
            Err(_) => return Err("Provider response timed out after 60s waiting for initial response (TTFB)".to_string()),
        }
    };
    let status = response.status();
    if !status.is_success() {
        let body = match read_bounded_response(response, &cancel, 1024 * 1024).await {
            Ok(body) => body,
            Err(_) if cancel.is_cancelled() => {
                return Ok(StreamResult {
                    cancelled: true,
                    ..StreamResult::default()
                })
            }
            Err(error) => return Err(error),
        };
        return Err(format!(
            "Provider returned HTTP {status}: {}",
            bounded_error(&body)
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !options.stream || !content_type.contains("text/event-stream") {
        let body = match read_bounded_response(response, &cancel, MAX_PROVIDER_RESPONSE_BYTES).await
        {
            Ok(body) => body,
            Err(_) if cancel.is_cancelled() => {
                return Ok(StreamResult {
                    cancelled: true,
                    ..StreamResult::default()
                })
            }
            Err(error) => return Err(error),
        };
        let value: serde_json::Value = serde_json::from_str(&body).map_err(|error| {
            format!("Expected JSON or SSE from provider, received invalid JSON: {error}")
        })?;
        let mut accumulator = StreamAccumulator::default();
        match provider.kind {
            ProviderKind::Anthropic => process_anthropic_message(
                &value,
                &mut accumulator,
                &mut on_chunk,
                &mut on_reasoning,
                &mut on_tool_delta,
            ),
            _ => process_openai_message(
                &value,
                &mut accumulator,
                &mut on_chunk,
                &mut on_reasoning,
                &mut on_tool_delta,
            ),
        }
        let result = accumulator.finish();
        validate_stream_result(&result)?;
        return Ok(result);
    }

    let mut accumulator = StreamAccumulator::default();
    let done = AtomicBool::new(false);
    let stream_error = Mutex::new(None::<String>);
    {
        let mut process_event = |data: &str| {
            if data.trim() == "[DONE]" {
                done.store(true, Ordering::Relaxed);
                return;
            }
            let value = match serde_json::from_str::<serde_json::Value>(data) {
                Ok(value) => value,
                Err(error) => {
                    if let Ok(mut slot) = stream_error.lock() {
                        *slot = Some(format!("Provider emitted malformed SSE JSON: {error}"));
                    }
                    done.store(true, Ordering::Relaxed);
                    return;
                }
            };
            if let Some(error) = value.get("error") {
                let message = error
                    .get("message")
                    .and_then(|v| v.as_str())
                    .or_else(|| error.as_str())
                    .unwrap_or("Provider emitted an error event");
                if let Ok(mut slot) = stream_error.lock() {
                    *slot = Some(message.to_string());
                }
                done.store(true, Ordering::Relaxed);
                return;
            }
            match provider.kind {
                ProviderKind::Anthropic => process_anthropic_event(
                    &value,
                    &mut accumulator,
                    &mut on_chunk,
                    &mut on_reasoning,
                    &mut on_tool_delta,
                ),
                _ => process_openai_event(
                    &value,
                    &mut accumulator,
                    &mut on_chunk,
                    &mut on_reasoning,
                    &mut on_tool_delta,
                ),
            }
        };
        let mut decoder = SseDecoder::default();
        let mut stream = response.bytes_stream();
        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => break,
                item = tokio::time::timeout(PROVIDER_STALE_TIMEOUT, stream.next()) => {
                    item.map_err(|_| "Provider stream timed out after 60s without data".to_string())?
                }
            };
            let Some(item) = next else { break };
            let bytes = item.map_err(|error| format!("Provider stream failed: {error}"))?;
            decoder.feed(&bytes, &mut process_event);
            if decoder.failed || done.load(Ordering::Relaxed) {
                break;
            }
        }
        if !cancel.is_cancelled() && !done.load(Ordering::Relaxed) {
            decoder.finish(&mut process_event);
        }
        // A decoder failure must stop parsing and surface an explicit error,
        // never a silent success. This is checked independently of the event
        // side-channel so the guarantee does not depend on `fail()` emitting.
        if decoder.failed {
            return Err(SSE_EVENT_TOO_LARGE.to_string());
        }
    }
    if let Some(error) = stream_error.into_inner().unwrap_or(None) {
        return Err(error);
    }
    accumulator.cancelled = cancel.is_cancelled();
    let result = accumulator.finish();
    validate_stream_result(&result)?;
    Ok(result)
}

fn openai_body(
    provider: &ProviderConfig,
    messages: &serde_json::Value,
    tools: &serde_json::Value,
    options: &CompletionOptions,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": provider.model, "messages": messages, "stream": options.stream,
        "temperature": options.temperature,
        "thinking": { "type": if options.thinking { "enabled" } else { "disabled" } },
        "reasoning_effort": options.reasoning_effort.as_str(),
    });
    if let Some(max_tokens) = options.max_tokens {
        body["max_tokens"] = max_tokens.into();
    }
    if tools.as_array().is_some_and(|items| !items.is_empty()) {
        body["tools"] = tools.clone();
        body["tool_choice"] = "auto".into();
    }
    if provider.kind == ProviderKind::Ollama {
        body["think"] = options.thinking.into();
    }
    merge_extra_body(&mut body, provider);
    body
}

fn anthropic_body(
    provider: &ProviderConfig,
    messages: &serde_json::Value,
    tools: &serde_json::Value,
    options: &CompletionOptions,
) -> Result<serde_json::Value, String> {
    let (system, converted) = convert_anthropic_messages(messages)?;
    let mut max_tokens = options.max_tokens.unwrap_or(8_192).max(1);
    if options.thinking {
        max_tokens = max_tokens.max(2_048);
    }
    let mut body = serde_json::json!({
        "model": provider.model, "messages": converted, "system": system, "stream": options.stream,
        "temperature": options.temperature, "max_tokens": max_tokens,
        "output_config": { "effort": options.reasoning_effort.as_str() },
    });
    body["thinking"] = if options.thinking {
        serde_json::json!({"type":"enabled","budget_tokens":options.reasoning_effort.anthropic_budget(max_tokens)})
    } else {
        serde_json::json!({"type":"disabled"})
    };
    if let Some(items) = tools.as_array().filter(|items| !items.is_empty()) {
        body["tools"] = serde_json::Value::Array(items.iter().filter_map(|tool| {
            let function = tool.get("function")?;
            Some(serde_json::json!({
                "name": function.get("name")?,
                "description": function.get("description").cloned().unwrap_or_default(),
                "input_schema": function.get("parameters").cloned().unwrap_or_else(|| serde_json::json!({"type":"object","properties":{}})),
            }))
        }).collect());
        body["tool_choice"] = serde_json::json!({"type":"auto"});
    }
    merge_extra_body(&mut body, provider);
    Ok(body)
}

fn merge_extra_body(body: &mut serde_json::Value, provider: &ProviderConfig) {
    if let Some(object) = body.as_object_mut() {
        for (key, value) in &provider.extra_body {
            object.insert(key.clone(), value.clone());
        }
    }
}

fn convert_anthropic_messages(
    messages: &serde_json::Value,
) -> Result<(String, Vec<serde_json::Value>), String> {
    let entries = messages.as_array().ok_or("Messages must be an array")?;
    let mut system = Vec::new();
    let mut output: Vec<serde_json::Value> = Vec::new();
    for message in entries {
        let role = message
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("user");
        let content = message
            .get("content")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if role == "system" {
            if let Some(text) = content.as_str() {
                system.push(text.to_string());
            }
            continue;
        }
        let (target_role, blocks) = match role {
            "assistant" => {
                let mut blocks = content_to_blocks(&content);
                if let Some(calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
                    for call in calls {
                        let id = call.get("id").and_then(|v| v.as_str()).unwrap_or("call");
                        let function = call.get("function");
                        let name = function
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        let input = function
                            .and_then(|f| f.get("arguments"))
                            .and_then(|v| v.as_str())
                            .and_then(|args| serde_json::from_str(args).ok())
                            .unwrap_or_else(|| serde_json::json!({}));
                        blocks.push(serde_json::json!({"type":"tool_use","id":id,"name":name,"input":input}));
                    }
                }
                ("assistant", blocks)
            }
            "tool" => (
                "user",
                vec![serde_json::json!({
                    "type":"tool_result", "tool_use_id":message.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("call"),
                    "content":text_from_value(&content),
                })],
            ),
            _ => ("user", content_to_blocks(&content)),
        };
        if let Some(last) = output.last_mut() {
            if last.get("role").and_then(|v| v.as_str()) == Some(target_role) {
                if let Some(existing) = last.get_mut("content").and_then(|v| v.as_array_mut()) {
                    existing.extend(blocks);
                    continue;
                }
            }
        }
        output.push(serde_json::json!({"role":target_role,"content":blocks}));
    }
    Ok((system.join("\n\n"), output))
}

fn content_to_blocks(content: &serde_json::Value) -> Vec<serde_json::Value> {
    match content {
        serde_json::Value::Array(items) => items.clone(),
        value => {
            let text = text_from_value(value);
            if text.is_empty() {
                Vec::new()
            } else {
                vec![serde_json::json!({"type":"text","text":text})]
            }
        }
    }
}

fn process_openai_event<F1, F2, F3>(
    value: &serde_json::Value,
    accumulator: &mut StreamAccumulator,
    on_chunk: &mut F1,
    on_reasoning: &mut F2,
    on_tool_delta: &mut F3,
) where
    F1: FnMut(String),
    F2: FnMut(String),
    F3: FnMut(ToolDelta),
{
    if let Some(choice) = value.pointer("/choices/0") {
        capture_finish_reason(accumulator, choice.get("finish_reason"));
        if let Some(delta) = choice.get("delta") {
            process_openai_delta(delta, accumulator, on_chunk, on_reasoning, on_tool_delta);
        }
    }
}

fn process_openai_message<F1, F2, F3>(
    value: &serde_json::Value,
    accumulator: &mut StreamAccumulator,
    on_chunk: &mut F1,
    on_reasoning: &mut F2,
    on_tool_delta: &mut F3,
) where
    F1: FnMut(String),
    F2: FnMut(String),
    F3: FnMut(ToolDelta),
{
    if let Some(choice) = value.pointer("/choices/0") {
        capture_finish_reason(accumulator, choice.get("finish_reason"));
        if let Some(message) = choice.get("message") {
            process_openai_delta(message, accumulator, on_chunk, on_reasoning, on_tool_delta);
        } else if let Some(delta) = choice.get("delta") {
            process_openai_delta(delta, accumulator, on_chunk, on_reasoning, on_tool_delta);
        }
    }
}

/// Records the provider's finish reason so a truncated or filtered run can be
/// told apart from a clean one.
fn capture_finish_reason(accumulator: &mut StreamAccumulator, value: Option<&serde_json::Value>) {
    if let Some(reason) = value
        .and_then(|value| value.as_str())
        .filter(|reason| !reason.is_empty())
    {
        accumulator.finish_reason = Some(reason.to_string());
    }
}

fn process_openai_delta<F1, F2, F3>(
    delta: &serde_json::Value,
    accumulator: &mut StreamAccumulator,
    on_chunk: &mut F1,
    on_reasoning: &mut F2,
    on_tool_delta: &mut F3,
) where
    F1: FnMut(String),
    F2: FnMut(String),
    F3: FnMut(ToolDelta),
{
    for key in ["reasoning_content", "reasoning"] {
        if let Some(reasoning) = delta
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
        {
            accumulator.reasoning.push_str(reasoning);
            on_reasoning(reasoning.to_string());
            break;
        }
    }
    let content = delta
        .get("content")
        .map(text_from_value)
        .unwrap_or_default();
    if !content.is_empty() {
        accumulator.content.push_str(&content);
        on_chunk(content);
    }
    if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
        for (position, call) in calls.iter().enumerate() {
            let index = call
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(position as u64) as usize;
            let id = call.get("id").and_then(|v| v.as_str()).map(str::to_string);
            let name = call
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let arguments = call
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            accumulate_tool(
                accumulator,
                index,
                id.clone(),
                name.clone(),
                arguments.clone(),
            );
            on_tool_delta(ToolDelta {
                index,
                id,
                name,
                arguments,
            });
        }
    }
}

fn process_anthropic_event<F1, F2, F3>(
    value: &serde_json::Value,
    accumulator: &mut StreamAccumulator,
    on_chunk: &mut F1,
    on_reasoning: &mut F2,
    on_tool_delta: &mut F3,
) where
    F1: FnMut(String),
    F2: FnMut(String),
    F3: FnMut(ToolDelta),
{
    match value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
    {
        "content_block_start" => {
            let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if let Some(block) = value.get("content_block") {
                match block
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                {
                    "text" => {
                        let text = block
                            .get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        if !text.is_empty() {
                            accumulator.content.push_str(text);
                            on_chunk(text.to_string());
                        }
                    }
                    "thinking" => {
                        let text = block
                            .get("thinking")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        if !text.is_empty() {
                            accumulator.reasoning.push_str(text);
                            on_reasoning(text.to_string());
                        }
                    }
                    "tool_use" => {
                        let id = block.get("id").and_then(|v| v.as_str()).map(str::to_string);
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        let arguments = block.get("input").and_then(|input| {
                            if input == &serde_json::json!({}) {
                                None
                            } else {
                                Some(input.to_string())
                            }
                        });
                        accumulate_tool(
                            accumulator,
                            index,
                            id.clone(),
                            name.clone(),
                            arguments.clone(),
                        );
                        on_tool_delta(ToolDelta {
                            index,
                            id,
                            name,
                            arguments,
                        });
                    }
                    _ => {}
                }
            }
        }
        "content_block_delta" => {
            let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let Some(delta) = value.get("delta") else {
                return;
            };
            match delta
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
            {
                "text_delta" => {
                    let text = delta
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if !text.is_empty() {
                        accumulator.content.push_str(text);
                        on_chunk(text.to_string());
                    }
                }
                "thinking_delta" => {
                    let text = delta
                        .get("thinking")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if !text.is_empty() {
                        accumulator.reasoning.push_str(text);
                        on_reasoning(text.to_string());
                    }
                }
                "input_json_delta" => {
                    let arguments = delta
                        .get("partial_json")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    accumulate_tool(accumulator, index, None, None, arguments.clone());
                    on_tool_delta(ToolDelta {
                        index,
                        id: None,
                        name: None,
                        arguments,
                    });
                }
                _ => {}
            }
        }
        "message_delta" => {
            // Anthropic reports the stop reason (end_turn / max_tokens /
            // tool_use / stop_sequence) here, not on the content deltas.
            let Some(delta) = value.get("delta") else {
                return;
            };
            capture_finish_reason(accumulator, delta.get("stop_reason"));
        }
        _ => {}
    }
}

fn process_anthropic_message<F1, F2, F3>(
    value: &serde_json::Value,
    accumulator: &mut StreamAccumulator,
    on_chunk: &mut F1,
    on_reasoning: &mut F2,
    on_tool_delta: &mut F3,
) where
    F1: FnMut(String),
    F2: FnMut(String),
    F3: FnMut(ToolDelta),
{
    capture_finish_reason(accumulator, value.get("stop_reason"));
    if let Some(blocks) = value.get("content").and_then(|v| v.as_array()) {
        for (index, block) in blocks.iter().enumerate() {
            if block.get("type").and_then(|value| value.as_str()) == Some("tool_use") {
                let id = block
                    .get("id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                let name = block
                    .get("name")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                let arguments = Some(
                    block
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}))
                        .to_string(),
                );
                accumulate_tool(
                    accumulator,
                    index,
                    id.clone(),
                    name.clone(),
                    arguments.clone(),
                );
                on_tool_delta(ToolDelta {
                    index,
                    id,
                    name,
                    arguments,
                });
            } else {
                let wrapped = serde_json::json!({"type":"content_block_start","index":index,"content_block":block});
                process_anthropic_event(
                    &wrapped,
                    accumulator,
                    on_chunk,
                    on_reasoning,
                    on_tool_delta,
                );
            }
        }
    }
}

fn accumulate_tool(
    accumulator: &mut StreamAccumulator,
    index: usize,
    id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
) {
    let entry = accumulator.tools.entry(index).or_default();
    if let Some(id) = id.filter(|value| !value.is_empty()) {
        entry.id = id;
    }
    if let Some(name) = name.filter(|value| !value.is_empty()) {
        entry.name = name;
    }
    if let Some(arguments) = arguments {
        entry.arguments.push_str(&arguments);
    }
}

fn text_from_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str().map(str::to_string).or_else(|| {
                    item.get("text")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
            })
            .collect(),
        _ => String::new(),
    }
}

fn request_headers(provider: &ProviderConfig) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "accept",
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    if !provider.api_key.is_empty() {
        let value = HeaderValue::from_str(&provider.api_key)
            .map_err(|_| "API key contains invalid header characters".to_string())?;
        if provider.kind == ProviderKind::Anthropic {
            headers.insert("x-api-key", value);
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        } else {
            let value = HeaderValue::from_str(&format!("Bearer {}", provider.api_key))
                .map_err(|_| "API key contains invalid header characters".to_string())?;
            headers.insert(AUTHORIZATION, value);
        }
    } else if provider.kind == ProviderKind::Anthropic {
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    }
    for (name, value) in &provider.extra_headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("Invalid extra header name: {name}"))?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| format!("Invalid value for extra header: {name}"))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

fn endpoint(base: &str, versioned: &str, suffix: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    if base.ends_with(suffix) {
        return base.to_string();
    }
    if base.ends_with("/v1") {
        format!("{base}{}", versioned.strip_prefix("/v1").unwrap_or(suffix))
    } else {
        format!("{base}{versioned}")
    }
}

fn bounded_error(body: &str) -> String {
    const MAX: usize = 4_000;
    if body.len() <= MAX {
        body.to_string()
    } else {
        let mut end = MAX;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}… [truncated]", &body[..end])
    }
}

/// Surfaced to the caller whenever the SSE decoder latches a failure, so a
/// stream that cannot be decoded is never reported as a successful run.
const SSE_EVENT_TOO_LARGE: &str = "Provider SSE event exceeded 1 MiB";

#[derive(Default)]
struct SseDecoder {
    buffer: Vec<u8>,
    data_lines: Vec<String>,
    data_bytes: usize,
    failed: bool,
}
impl SseDecoder {
    const MAX_EVENT_BYTES: usize = 1024 * 1024;

    fn fail<F>(&mut self, on_event: &mut F)
    where
        F: FnMut(&str),
    {
        if self.failed {
            return;
        }
        self.failed = true;
        self.buffer.clear();
        self.data_lines.clear();
        self.data_bytes = 0;
        on_event(&format!(
            "{{\"error\":{{\"message\":\"{SSE_EVENT_TOO_LARGE}\"}}}}"
        ));
    }

    fn feed<F>(&mut self, bytes: &[u8], on_event: &mut F)
    where
        F: FnMut(&str),
    {
        if self.failed {
            return;
        }
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > Self::MAX_EVENT_BYTES && !self.buffer.contains(&b'\n') {
            self.fail(on_event);
            return;
        }
        while !self.failed {
            let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') else {
                break;
            };
            let line = self.buffer.drain(..=position).collect::<Vec<_>>();
            self.consume_line(&line[..line.len().saturating_sub(1)], on_event);
        }
        if self.failed {
            self.buffer.clear();
        }
    }
    fn finish<F>(&mut self, on_event: &mut F)
    where
        F: FnMut(&str),
    {
        if self.failed {
            return;
        }
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            self.consume_line(&line, on_event);
        }
        self.dispatch(on_event);
    }
    fn consume_line<F>(&mut self, raw: &[u8], on_event: &mut F)
    where
        F: FnMut(&str),
    {
        if self.failed {
            return;
        }
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let Ok(line) = std::str::from_utf8(raw) else {
            return;
        };
        if line.is_empty() {
            self.dispatch(on_event);
        } else if let Some(data) = line.strip_prefix("data:") {
            let data = data.strip_prefix(' ').unwrap_or(data);
            if self.data_bytes + data.len() > Self::MAX_EVENT_BYTES {
                self.fail(on_event);
                return;
            }
            self.data_bytes += data.len();
            self.data_lines.push(data.to_string());
        }
    }
    fn dispatch<F>(&mut self, on_event: &mut F)
    where
        F: FnMut(&str),
    {
        if !self.data_lines.is_empty() {
            let event = self.data_lines.join("\n");
            self.data_lines.clear();
            self.data_bytes = 0;
            on_event(&event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_does_not_duplicate_v1() {
        assert_eq!(
            endpoint("https://x.test/v1", "/v1/models", "/models"),
            "https://x.test/v1/models"
        );
        assert_eq!(
            endpoint("https://x.test", "/v1/models", "/models"),
            "https://x.test/v1/models"
        );
    }
    #[test]
    fn sse_handles_crlf_multiline_and_split_utf8() {
        let bytes = "data: {\"text\":\"héllo\"}\r\n\r\n".as_bytes();
        let split = bytes
            .windows(2)
            .position(|pair| pair == [0xC3, 0xA9])
            .unwrap_or(5)
            + 1;
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        decoder.feed(&bytes[..split], &mut |event| events.push(event.to_string()));
        decoder.feed(&bytes[split..], &mut |event| events.push(event.to_string()));
        assert_eq!(events, vec!["{\"text\":\"héllo\"}"]);
    }

    #[test]
    fn sse_rejects_unbounded_events() {
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        decoder.feed(&vec![b'a'; SseDecoder::MAX_EVENT_BYTES + 1], &mut |event| {
            events.push(event.to_string())
        });
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("exceeded 1 MiB"));
    }

    #[test]
    fn sse_decoder_stops_processing_after_a_failure() {
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        // One oversized event followed by a malformed line, delivered in a
        // single feed() call: the malformed line must not be dispatched, or it
        // masks the real cause with a generic parse error.
        let mut payload = format!("data: {}\n\n", "a".repeat(SseDecoder::MAX_EVENT_BYTES + 1));
        payload.push_str("data: not-json\n\n");
        decoder.feed(payload.as_bytes(), &mut |event| {
            events.push(event.to_string())
        });
        assert!(
            decoder.failed,
            "an oversized event must latch the failure flag"
        );
        assert_eq!(
            events.len(),
            1,
            "no further events may be dispatched after a failure: {events:?}"
        );
        assert!(
            events[0].contains("exceeded 1 MiB"),
            "the size failure must be the reported cause: {:?}",
            events[0]
        );

        // Later chunks must be ignored entirely.
        decoder.feed(b"data: not-json\n\n", &mut |event| {
            events.push(event.to_string())
        });
        decoder.finish(&mut |event| events.push(event.to_string()));
        assert_eq!(
            events.len(),
            1,
            "no further events after a failure: {events:?}"
        );
        assert!(decoder.failed);
    }

    #[tokio::test]
    async fn openai_http_stream_surfaces_oversized_sse_event_as_error() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut scratch = [0u8; 1024];
            let header_end;
            loop {
                let count = stream.read(&mut scratch).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&scratch[..count]);
                if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    header_end = position + 4;
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let count = stream.read(&mut scratch).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&scratch[..count]);
            }
            let _body: serde_json::Value =
                serde_json::from_slice(&request[header_end..header_end + content_length]).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            // An event past the 1 MiB cap, followed by trailing lines. The
            // trailing malformed line must not mask the size failure.
            let mut payload = format!("data: {}\n\n", "a".repeat(SseDecoder::MAX_EVENT_BYTES + 1));
            payload.push_str("data: trailing-garbage\n\n");
            for chunk in payload.as_bytes().chunks(64 * 1024) {
                stream.write_all(chunk).await.unwrap();
                stream.flush().await.unwrap();
            }
        });

        let provider = ProviderConfig {
            base_url: format!("http://{address}"),
            model: "test-model".into(),
            ..ProviderConfig::default()
        };
        let result = stream_chat_completion(
            &provider,
            &serde_json::json!([{"role":"user","content":"test"}]),
            &serde_json::json!([]),
            &CompletionOptions {
                thinking: false,
                reasoning_effort: ReasoningEffort::Low,
                temperature: 0.3,
                max_tokens: Some(512),
                stream: true,
            },
            |_| {},
            |_| {},
            |_| {},
            CancellationToken::new(),
        )
        .await;

        let error = result.expect_err("an oversized SSE event must fail the stream");
        assert!(
            error.contains("exceeded 1 MiB"),
            "the size failure must reach the caller, got: {error}"
        );
        server.await.unwrap();
    }

    #[test]
    fn openai_deltas_accumulate_reasoning_content_and_tools() {
        let mut accumulator = StreamAccumulator::default();
        let mut text = Vec::new();
        let mut reasoning = Vec::new();
        let mut deltas = Vec::new();
        for value in [
            serde_json::json!({"choices":[{"delta":{"reasoning_content":"think","tool_calls":[{"index":0,"id":"call1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}),
            serde_json::json!({"choices":[{"delta":{"content":"answer","tool_calls":[{"index":0,"function":{"arguments":"th\":\"x\"}"}}]}}]}),
        ] {
            process_openai_event(
                &value,
                &mut accumulator,
                &mut |v| text.push(v),
                &mut |v| reasoning.push(v),
                &mut |v| deltas.push(v),
            );
        }
        let result = accumulator.finish();
        assert_eq!(result.content, "answer");
        assert_eq!(result.reasoning, "think");
        assert_eq!(result.tool_calls[0].function.arguments, "{\"path\":\"x\"}");
        assert_eq!(result.tool_calls[0].stream_index, 0);
        assert_eq!(deltas.len(), 2);
    }
    #[test]
    fn anthropic_deltas_accumulate_thinking_and_tool_json() {
        let mut accumulator = StreamAccumulator::default();
        for event in [
            serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"write_file","input":{}}}),
            serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"a\"}"}}),
            serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"thinking_delta","thinking":"reason"}}),
        ] {
            process_anthropic_event(
                &event,
                &mut accumulator,
                &mut |_| {},
                &mut |_| {},
                &mut |_| {},
            );
        }
        let result = accumulator.finish();
        assert_eq!(result.reasoning, "reason");
        assert_eq!(result.tool_calls[0].id, "toolu_1");
        assert_eq!(result.tool_calls[0].function.arguments, "{\"path\":\"a\"}");
    }
    #[test]
    fn request_contains_thinking_and_effort() {
        let provider = ProviderConfig {
            model: "m".into(),
            ..ProviderConfig::default()
        };
        let body = openai_body(
            &provider,
            &serde_json::json!([]),
            &serde_json::json!([]),
            &CompletionOptions {
                thinking: true,
                reasoning_effort: ReasoningEffort::ExtraHigh,
                temperature: 0.2,
                max_tokens: Some(1000),
                stream: true,
            },
        );
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "extra_high");
        assert_eq!(body["max_tokens"], 1000);
    }

    /// Spins up a one-shot HTTP server that replies with `sse_body` and returns
    /// the JSON request body it received.
    async fn sse_server(sse_body: String) -> (String, tokio::task::JoinHandle<serde_json::Value>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut scratch = [0u8; 1024];
            let header_end;
            loop {
                let count = stream.read(&mut scratch).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&scratch[..count]);
                if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    header_end = position + 4;
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let count = stream.read(&mut scratch).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&scratch[..count]);
            }
            let body: serde_json::Value =
                serde_json::from_slice(&request[header_end..header_end + content_length]).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            for chunk in sse_body.as_bytes().chunks(7) {
                stream.write_all(chunk).await.unwrap();
                stream.flush().await.unwrap();
            }
            body
        });
        (format!("http://{address}"), server)
    }

    async fn stream_against(base_url: &str) -> Result<StreamResult, String> {
        let provider = ProviderConfig {
            base_url: base_url.to_string(),
            model: "test-model".into(),
            ..ProviderConfig::default()
        };
        stream_chat_completion(
            &provider,
            &serde_json::json!([{"role":"user","content":"test"}]),
            &serde_json::json!([]),
            &CompletionOptions {
                thinking: false,
                reasoning_effort: ReasoningEffort::Low,
                temperature: 0.3,
                max_tokens: Some(512),
                stream: true,
            },
            |_| {},
            |_| {},
            |_| {},
            CancellationToken::new(),
        )
        .await
    }

    /// A stream cut short by the token limit must not look like a clean success.
    #[tokio::test]
    async fn openai_http_stream_reports_truncated_finish_reason_as_error() {
        let (base_url, server) = sse_server(concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string())
        .await;

        let error = stream_against(&base_url)
            .await
            .expect_err("a truncated stream must not be reported as success");

        assert!(
            error.contains("length"),
            "the finish_reason must reach the caller, got: {error}"
        );
        assert!(
            error.to_ascii_lowercase().contains("truncat")
                || error.to_ascii_lowercase().contains("finish_reason"),
            "the error must explain the truncation, got: {error}"
        );
        let _ = server.await.unwrap();
    }

    /// Content-filtered or otherwise abnormal stops must also be surfaced.
    #[tokio::test]
    async fn openai_http_stream_reports_content_filter_finish_reason_as_error() {
        let (base_url, server) = sse_server(concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"content_filter\"}]}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string())
        .await;

        let error = stream_against(&base_url)
            .await
            .expect_err("a content-filtered stream must not be reported as success");
        assert!(error.contains("content_filter"), "got: {error}");
        let _ = server.await.unwrap();
    }

    /// A clean stop with content must keep working (no false positives).
    #[tokio::test]
    async fn openai_http_stream_accepts_stop_finish_reason() {
        let (base_url, server) = sse_server(concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"full answer\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string())
        .await;

        let result = stream_against(&base_url)
            .await
            .expect("a normally finished stream must succeed");
        assert_eq!(result.content, "full answer");
        assert_eq!(result.finish_reason.as_deref(), Some("stop"));
        let _ = server.await.unwrap();
    }

    /// A tool-call stop is a normal completion, not a truncation.
    #[tokio::test]
    async fn openai_http_stream_accepts_tool_calls_finish_reason() {
        let (base_url, server) = sse_server(concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string())
        .await;

        let result = stream_against(&base_url)
            .await
            .expect("a tool-call stream must succeed");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.finish_reason.as_deref(), Some("tool_calls"));
        let _ = server.await.unwrap();
    }

    /// A stream that produces nothing at all must not be reported as success.
    #[tokio::test]
    async fn openai_http_stream_rejects_empty_completion() {
        let (base_url, server) = sse_server("data: [DONE]\n\n".to_string()).await;

        let error = stream_against(&base_url)
            .await
            .expect_err("an empty stream must not be reported as success");
        assert!(
            error.to_ascii_lowercase().contains("no content")
                || error.to_ascii_lowercase().contains("produced no"),
            "got: {error}"
        );
        let _ = server.await.unwrap();
    }

    #[test]
    fn finish_reason_validation_distinguishes_clean_and_truncated_runs() {
        let clean = |reason: &str| StreamResult {
            content: "answer".into(),
            finish_reason: Some(reason.into()),
            ..StreamResult::default()
        };
        for reason in [
            "stop",
            "tool_calls",
            "function_call",
            "end_turn",
            "tool_use",
            "stop_sequence",
        ] {
            assert!(
                validate_stream_result(&clean(reason)).is_ok(),
                "{reason} is a normal completion"
            );
        }
        for reason in ["length", "max_tokens", "content_filter"] {
            assert!(
                validate_stream_result(&clean(reason)).is_err(),
                "{reason} must be surfaced"
            );
        }

        // No finish_reason reported at all is tolerated (many providers omit it).
        assert!(validate_stream_result(&StreamResult {
            content: "answer".into(),
            ..StreamResult::default()
        })
        .is_ok());

        // Nothing produced at all is a silent failure.
        assert!(validate_stream_result(&StreamResult::default()).is_err());

        // Cancelled runs are never validated.
        assert!(validate_stream_result(&StreamResult {
            cancelled: true,
            ..StreamResult::default()
        })
        .is_ok());
    }

    #[test]
    fn anthropic_stop_reason_is_captured_for_validation() {
        let mut accumulator = StreamAccumulator::default();
        for event in [
            serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}),
            serde_json::json!({"type":"message_delta","delta":{"stop_reason":"max_tokens","stop_sequence":null}}),
        ] {
            process_anthropic_event(
                &event,
                &mut accumulator,
                &mut |_| {},
                &mut |_| {},
                &mut |_| {},
            );
        }
        let result = accumulator.finish();
        assert_eq!(result.content, "hi");
        assert_eq!(result.finish_reason.as_deref(), Some("max_tokens"));
        assert!(validate_stream_result(&result).is_err());
    }

    #[tokio::test]
    async fn openai_http_stream_round_trip_parses_network_chunks() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut scratch = [0u8; 1024];
            let header_end;
            loop {
                let count = stream.read(&mut scratch).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&scratch[..count]);
                if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    header_end = position + 4;
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let count = stream.read(&mut scratch).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&scratch[..count]);
            }
            let body: serde_json::Value =
                serde_json::from_slice(&request[header_end..header_end + content_length]).unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
            let events = concat!(
                "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"network-think\"}}]}\r\n\r\n",
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_net\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"network-answer\",\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]}}]}\n\n",
                "data: [DONE]\n\n"
            );
            for chunk in events.as_bytes().chunks(17) {
                stream.write_all(chunk).await.unwrap();
                stream.flush().await.unwrap();
            }
            body
        });

        let provider = ProviderConfig {
            base_url: format!("http://{address}"),
            model: "test-model".into(),
            ..ProviderConfig::default()
        };
        let mut text = String::new();
        let mut reasoning = String::new();
        let result = stream_chat_completion(
            &provider,
            &serde_json::json!([{"role":"user","content":"test"}]),
            &serde_json::json!([{"type":"function","function":{"name":"read_file","parameters":{"type":"object"}}}]),
            &CompletionOptions { thinking: true, reasoning_effort: ReasoningEffort::High, temperature: 0.3, max_tokens: Some(512), stream: true },
            |chunk| text.push_str(&chunk),
            |chunk| reasoning.push_str(&chunk),
            |_| {},
            CancellationToken::new(),
        ).await.unwrap();
        let request = server.await.unwrap();
        assert_eq!(request["model"], "test-model");
        assert_eq!(request["thinking"]["type"], "enabled");
        assert_eq!(request["reasoning_effort"], "high");
        assert_eq!(result.content, "network-answer");
        assert_eq!(result.reasoning, "network-think");
        assert_eq!(
            result.tool_calls[0].function.arguments,
            "{\"path\":\"a.txt\"}"
        );
        assert_eq!(text, "network-answer");
        assert_eq!(reasoning, "network-think");
    }

    #[test]
    fn retryable_provider_error_classification() {
        // Retryable errors:
        assert!(is_retryable_provider_error("Provider stream timed out after 60s without data"));
        assert!(is_retryable_provider_error("Provider response timed out after 60s waiting for initial response (TTFB)"));
        assert!(is_retryable_provider_error("Provider returned HTTP 500: Internal Server Error"));
        assert!(is_retryable_provider_error("Provider returned HTTP 502: Bad Gateway"));
        assert!(is_retryable_provider_error("Provider returned HTTP 503: Service Unavailable"));
        assert!(is_retryable_provider_error("Provider returned HTTP 504: Gateway Timeout"));
        assert!(is_retryable_provider_error("Provider returned HTTP 429: Rate limit exceeded"));
        assert!(is_retryable_provider_error("Provider stream failed: connection reset by peer"));
        assert!(is_retryable_provider_error("Provider request failed: error sending request"));
        assert!(is_retryable_provider_error("Provider stream produced no content, reasoning, or tool calls before completing"));

        // Non-retryable errors:
        assert!(!is_retryable_provider_error("Provider returned HTTP 400: Bad Request"));
        assert!(!is_retryable_provider_error("Provider returned HTTP 401: Invalid API Key"));
        assert!(!is_retryable_provider_error("Provider returned HTTP 403: Forbidden"));
        assert!(!is_retryable_provider_error("Provider returned HTTP 404: Not Found"));
        assert!(!is_retryable_provider_error("Provider response cancelled"));
        assert!(!is_retryable_provider_error("A model must be selected"));
        assert!(!is_retryable_provider_error("Provider base URL is required"));
        assert!(!is_retryable_provider_error("Provider stream ended with finish_reason 'length'"));
    }
}
