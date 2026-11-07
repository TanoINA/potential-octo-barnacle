use super::types::McpServerConfig;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{oneshot, Mutex};

struct Shared {
    stdin: Mutex<ChildStdin>,
    pending: StdMutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>>,
    next_id: AtomicU64,
}

#[derive(Clone)]
pub struct McpClient {
    shared: Arc<Shared>,
    child: Arc<Mutex<Child>>,
}

impl McpClient {
    pub async fn connect(config: &McpServerConfig, cwd: &str) -> Result<Self, String> {
        if config.command.trim().is_empty() {
            return Err("MCP server command must not be empty".into());
        }
        let mut command = super::create_hidden_command(&config.command);
        command
            .args(&config.args)
            .envs(&config.env)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start MCP server '{}': {error}", config.name))?;
        let stdin = child
            .stdin
            .take()
            .ok_or("MCP server stdin was not available")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("MCP server stdout was not available")?;
        let shared = Arc::new(Shared {
            stdin: Mutex::new(stdin),
            pending: StdMutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });
        let reader_shared = shared.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let Some(id) = message.get("id").and_then(|value| value.as_u64()) else {
                    continue;
                };
                let sender = reader_shared
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(&id));
                if let Some(sender) = sender {
                    if let Some(error) = message.get("error") {
                        let text = error
                            .get("message")
                            .and_then(|value| value.as_str())
                            .unwrap_or("MCP server returned an error");
                        let _ = sender.send(Err(text.to_string()));
                    } else {
                        let _ = sender.send(Ok(message
                            .get("result")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null)));
                    }
                }
            }
            if let Ok(mut pending) = reader_shared.pending.lock() {
                for (_, sender) in pending.drain() {
                    let _ = sender.send(Err("MCP server closed stdio".into()));
                }
            }
        });
        let client = Self {
            shared,
            child: Arc::new(Mutex::new(child)),
        };
        client
            .request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "clientInfo": { "name": "AetherAgent", "version": "0.2.0" }
                }),
            )
            .await?;
        client
            .notify("notifications/initialized", serde_json::json!({}))
            .await?;
        Ok(client)
    }

    async fn write(&self, mut value: serde_json::Value) -> Result<(), String> {
        value["jsonrpc"] = "2.0".into();
        let mut encoded = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
        encoded.push(b'\n');
        let mut stdin = self.shared.stdin.lock().await;
        stdin
            .write_all(&encoded)
            .await
            .map_err(|error| format!("MCP stdio write failed: {error}"))?;
        stdin
            .flush()
            .await
            .map_err(|error| format!("MCP stdio flush failed: {error}"))
    }

    pub async fn notify(&self, method: &str, params: serde_json::Value) -> Result<(), String> {
        self.write(serde_json::json!({ "method": method, "params": params }))
            .await
    }

    pub async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.shared
            .pending
            .lock()
            .map_err(|_| "MCP pending-request lock was poisoned".to_string())?
            .insert(id, sender);
        if let Err(error) = self
            .write(serde_json::json!({ "id": id, "method": method, "params": params }))
            .await
        {
            if let Ok(mut pending) = self.shared.pending.lock() {
                pending.remove(&id);
            }
            return Err(error);
        }
        match tokio::time::timeout(Duration::from_secs(20), receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                if let Ok(mut pending) = self.shared.pending.lock() {
                    pending.remove(&id);
                }
                Err("MCP response channel closed".into())
            }
            Err(_) => {
                if let Ok(mut pending) = self.shared.pending.lock() {
                    pending.remove(&id);
                }
                Err(format!("MCP request '{method}' timed out after 20s"))
            }
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<serde_json::Value>, String> {
        let result = self.request("tools/list", serde_json::json!({})).await?;
        Ok(result
            .get("tools")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default())
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": arguments }),
        )
        .await
    }

    pub async fn shutdown(&self) {
        let mut child = self.child.lock().await;
        if child.id().is_some() {
            let _ = child.kill().await;
        }
    }
}

pub struct McpManager {
    clients: Vec<McpClient>,
    calls: HashMap<String, (McpClient, String)>,
    pub tools: Vec<serde_json::Value>,
    pub warnings: Vec<String>,
}

impl McpManager {
    pub async fn connect(configs: &[McpServerConfig], cwd: &str) -> Self {
        let mut manager = Self {
            clients: Vec::new(),
            calls: HashMap::new(),
            tools: Vec::new(),
            warnings: Vec::new(),
        };
        for config in configs.iter().filter(|config| config.enabled) {
            match McpClient::connect(config, cwd).await {
                Ok(client) => match client.list_tools().await {
                    Ok(tools) => {
                        for tool in tools {
                            let Some(original_name) =
                                tool.get("name").and_then(|value| value.as_str())
                            else {
                                continue;
                            };
                            let base_alias = build_alias(&config.id, original_name, None);
                            let mut alias = base_alias.clone();
                            let mut suffix = 2;
                            while manager.calls.contains_key(&alias) {
                                alias = build_alias(&config.id, original_name, Some(suffix));
                                suffix += 1;
                            }
                            let description = tool
                                .get("description")
                                .and_then(|value| value.as_str())
                                .unwrap_or("External MCP tool");
                            let parameters = tool
                                .get("inputSchema")
                                .or_else(|| tool.get("input_schema"))
                                .cloned()
                                .unwrap_or_else(
                                    || serde_json::json!({"type":"object","properties":{}}),
                                );
                            manager.tools.push(serde_json::json!({
                                "type": "function",
                                "function": {
                                    "name": alias,
                                    "description": format!("[MCP: {}] {}", config.name, description),
                                    "parameters": parameters,
                                }
                            }));
                            manager
                                .calls
                                .insert(alias, (client.clone(), original_name.to_string()));
                        }
                        manager.clients.push(client);
                    }
                    Err(error) => {
                        let _ = client.shutdown().await;
                        manager
                            .warnings
                            .push(format!("MCP '{}': {error}", config.name));
                    }
                },
                Err(error) => manager.warnings.push(error),
            }
        }
        manager
    }

    pub fn contains(&self, name: &str) -> bool {
        self.calls.contains_key(name)
    }

    pub async fn call(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let (client, original_name) = self
            .calls
            .get(name)
            .ok_or_else(|| format!("Unknown MCP tool alias: {name}"))?;
        client.call_tool(original_name, arguments).await
    }

    pub async fn shutdown(&self) {
        for client in &self.clients {
            client.shutdown().await;
        }
    }
}

pub fn result_text(value: &serde_json::Value) -> String {
    if let Some(content) = value.get("content").and_then(|value| value.as_array()) {
        let text = content
            .iter()
            .filter_map(|item| item.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return text;
        }
    }
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| "MCP returned an unserializable result".into())
}

fn sanitize_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "server".into()
    } else {
        trimmed.chars().take(48).collect()
    }
}

/// Providers (OpenAI, Anthropic, ...) reject function names longer than 64
/// characters, which would fail the whole request. Native tool names are all
/// short, so MCP aliases have to be capped explicitly.
const MAX_ALIAS_CHARS: usize = 64;
/// Room reserved for the longest de-duplication suffix (`_9999`).
const ALIAS_SUFFIX_CHARS: usize = 5;

fn build_alias(server_id: &str, tool_name: &str, suffix: Option<u32>) -> String {
    let base = format!(
        "mcp__{}__{}",
        sanitize_name(server_id),
        sanitize_name(tool_name)
    );
    match suffix {
        None => base.chars().take(MAX_ALIAS_CHARS).collect(),
        Some(suffix) => {
            // Reserve room for the suffix before capping, so a suffixed alias
            // can never push the name back over the limit.
            let head: String = base
                .chars()
                .take(MAX_ALIAS_CHARS - ALIAS_SUFFIX_CHARS)
                .collect();
            let suffix = format!("_{suffix}");
            let keep = MAX_ALIAS_CHARS.saturating_sub(suffix.chars().count());
            let head: String = head.chars().take(keep).collect();
            format!("{head}{suffix}")
        }
    }
}

pub async fn list_server_tools(
    config: &McpServerConfig,
    cwd: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let client = McpClient::connect(config, cwd).await?;
    let result = client.list_tools().await;
    client.shutdown().await;
    result
}

pub async fn call_server_tool(
    config: &McpServerConfig,
    cwd: &str,
    name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = McpClient::connect(config, cwd).await?;
    let result = client.call_tool(name, arguments).await;
    client.shutdown().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_are_provider_safe() {
        assert_eq!(sanitize_name("My Server!"), "my_server");
        assert_eq!(sanitize_name("***"), "server");
    }

    #[test]
    fn extracts_mcp_text_content() {
        let value = serde_json::json!({"content":[{"type":"text","text":"one"},{"type":"text","text":"two"}]});
        assert_eq!(result_text(&value), "one\ntwo");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn json_rpc_stdio_round_trip_with_real_child_process() {
        let script = r#"
const rl = require('readline').createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const m = JSON.parse(line);
  const send = (result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }) + '\n');
  if (m.method === 'initialize') send({ protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'mock', version: '1' } });
  if (m.method === 'tools/list') send({ tools: [{ name: 'echo', description: 'Echo text', inputSchema: { type: 'object', properties: { text: { type: 'string' } } } }] });
  if (m.method === 'tools/call') send({ content: [{ type: 'text', text: m.params.arguments.text }] });
});
"#;
        let config = McpServerConfig {
            id: "mock".into(),
            name: "Mock MCP".into(),
            command: "node".into(),
            args: vec!["-e".into(), script.into()],
            env: HashMap::new(),
            enabled: true,
        };
        let cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let client = McpClient::connect(&config, &cwd).await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools[0]["name"], "echo");
        let result = client
            .call_tool("echo", serde_json::json!({"text":"mcp-ok"}))
            .await
            .unwrap();
        assert_eq!(result_text(&result), "mcp-ok");
        client.shutdown().await;
    }

    /// Mock MCP server exercising alias de-duplication, schema forwarding and
    /// server-side errors.
    const AUDIT_SERVER_SCRIPT: &str = r#"
const rl = require('readline').createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const m = JSON.parse(line);
  const send = (result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }) + '\n');
  const fail = (message) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, error: { code: -32000, message } }) + '\n');
  if (m.method === 'initialize') send({ protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'mock', version: '1' } });
  if (m.method === 'tools/list') send({ tools: [
    { name: 'echo', description: 'Echo text back', inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] } },
    { name: 'Echo Text', description: 'Second echo', inputSchema: { type: 'object', properties: { text: { type: 'string' } } } },
    { name: 'echo-text', description: 'Third echo', inputSchema: { type: 'object', properties: { extra: { type: 'boolean' } } } },
    { name: 'boom', description: 'Always fails', inputSchema: { type: 'object', properties: {} } },
    { name: 'no-schema', description: 'Missing schema' }
  ]});
  if (m.method === 'tools/call') {
    if (m.params.name === 'boom') fail('boom exploded');
    else send({ content: [{ type: 'text', text: 'called:' + m.params.name }] });
  }
});
"#;

    fn audit_server_config() -> McpServerConfig {
        McpServerConfig {
            id: "mock".into(),
            name: "Mock MCP".into(),
            command: "node".into(),
            args: vec!["-e".into(), AUDIT_SERVER_SCRIPT.into()],
            env: HashMap::new(),
            enabled: true,
        }
    }

    fn audit_cwd() -> String {
        std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    fn published_alias(tool: &serde_json::Value) -> String {
        tool.pointer("/function/name")
            .and_then(|value| value.as_str())
            .expect("published tool must expose a function name")
            .to_string()
    }

    /// (a) The alias advertised to the model must be exactly the alias the
    /// dispatcher accepts, including de-duplicated names.
    /// (b) Description and JSON schema must survive publication.
    /// (c) MCP server errors must reach the caller as a plain error string,
    /// the same shape native tool errors use.
    #[cfg(windows)]
    #[tokio::test]
    async fn mcp_aliases_schema_and_errors_match_the_native_tool_contract() {
        let manager = McpManager::connect(&[audit_server_config()], &audit_cwd()).await;
        assert!(
            manager.warnings.is_empty(),
            "unexpected connect warnings: {:?}",
            manager.warnings
        );

        // --- (a) published alias == accepted alias -------------------------
        let published: Vec<String> = manager.tools.iter().map(published_alias).collect();
        assert_eq!(
            published,
            vec![
                "mcp__mock__echo",
                "mcp__mock__echo_text",
                "mcp__mock__echo_text_2",
                "mcp__mock__boom",
                "mcp__mock__no_schema",
            ],
            "alias generation and de-duplication must be deterministic"
        );
        for alias in &published {
            assert!(
                manager.contains(alias),
                "dispatcher must accept the published alias {alias}"
            );
        }
        assert!(!manager.contains("mcp__mock__echo_text_3"));
        assert!(!manager.contains("echo"), "raw MCP names must not leak");

        // Each alias must route back to its own original tool name.
        for (alias, expected_original) in [
            ("mcp__mock__echo", "echo"),
            ("mcp__mock__echo_text", "Echo Text"),
            ("mcp__mock__echo_text_2", "echo-text"),
        ] {
            let value = manager
                .call(alias, serde_json::json!({"text": "x"}))
                .await
                .unwrap_or_else(|error| panic!("{alias} must be callable: {error}"));
            assert_eq!(
                result_text(&value),
                format!("called:{expected_original}"),
                "{alias} must dispatch to its own original tool"
            );
        }

        // --- (b) description and schema forwarded intact -------------------
        let find = |alias: &str| {
            manager
                .tools
                .iter()
                .find(|tool| published_alias(tool) == alias)
                .unwrap_or_else(|| panic!("missing published tool {alias}"))
                .clone()
        };
        let echo = find("mcp__mock__echo");
        let description = echo["function"]["description"].as_str().unwrap();
        assert!(
            description.contains("Echo text back"),
            "original description must survive: {description}"
        );
        assert!(
            description.contains("Mock MCP"),
            "server attribution must be present: {description}"
        );
        assert_eq!(
            echo["function"]["parameters"],
            serde_json::json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            "inputSchema must be forwarded verbatim"
        );
        // A schema that only differs per tool must not be shared or dropped.
        assert_eq!(
            find("mcp__mock__echo_text_2")["function"]["parameters"],
            serde_json::json!({"type":"object","properties":{"extra":{"type":"boolean"}}}),
        );
        // A missing inputSchema must fall back to an empty object schema.
        assert_eq!(
            find("mcp__mock__no_schema")["function"]["parameters"],
            serde_json::json!({"type":"object","properties":{}}),
        );
        // Every published tool must carry the same envelope as native tools.
        for tool in &manager.tools {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["parameters"].is_object());
        }

        // --- (c) MCP server errors ----------------------------------------
        let error = manager
            .call("mcp__mock__boom", serde_json::json!({}))
            .await
            .expect_err("a failing MCP tool must surface an error");
        assert_eq!(
            error, "boom exploded",
            "the server message must reach the caller unwrapped"
        );
        // The same call through an unknown alias is refused, like native tools.
        let unknown = manager
            .call("mcp__mock__nope", serde_json::json!({}))
            .await
            .expect_err("unknown aliases must be refused");
        assert!(unknown.contains("mcp__mock__nope"), "{unknown}");

        manager.shutdown().await;
    }

    /// Native tool names are short; MCP aliases are built from the server id and
    /// the tool name, so they can grow far past what providers accept (OpenAI
    /// and Anthropic both cap function names at 64 characters and reject the
    /// whole request otherwise).
    #[cfg(windows)]
    #[tokio::test]
    async fn mcp_aliases_stay_within_the_provider_function_name_limit() {
        let long_id = "a".repeat(80);
        let long_name = "b".repeat(80);
        let other_long_name = "c".repeat(80);
        let script = format!(
            r#"
const rl = require('readline').createInterface({{ input: process.stdin }});
rl.on('line', (line) => {{
  const m = JSON.parse(line);
  const send = (result) => process.stdout.write(JSON.stringify({{ jsonrpc: '2.0', id: m.id, result }}) + '\n');
  if (m.method === 'initialize') send({{ protocolVersion: '2024-11-05', capabilities: {{ tools: {{}} }}, serverInfo: {{ name: 'mock', version: '1' }} }});
  if (m.method === 'tools/list') send({{ tools: [
    {{ name: '{long_name}', description: 'Long tool', inputSchema: {{ type: 'object', properties: {{}} }} }},
    {{ name: '{other_long_name}', description: 'Colliding long tool', inputSchema: {{ type: 'object', properties: {{}} }} }}
  ]}});
  if (m.method === 'tools/call') send({{ content: [{{ type: 'text', text: 'called:' + m.params.name }}] }});
}});
"#
        );
        let config = McpServerConfig {
            id: long_id.clone(),
            name: "Long MCP".into(),
            command: "node".into(),
            args: vec!["-e".into(), script],
            env: HashMap::new(),
            enabled: true,
        };

        let manager = McpManager::connect(&[config], &audit_cwd()).await;
        assert!(
            manager.warnings.is_empty(),
            "unexpected warnings: {:?}",
            manager.warnings
        );

        let published: Vec<String> = manager.tools.iter().map(published_alias).collect();
        assert_eq!(published.len(), 2);
        for alias in &published {
            assert!(
                alias.chars().count() <= 64,
                "alias {alias} ({} chars) exceeds the provider function-name limit",
                alias.chars().count()
            );
            assert!(
                manager.contains(alias),
                "the shortened alias must still be the one the dispatcher accepts"
            );
            let value = manager
                .call(alias, serde_json::json!({}))
                .await
                .unwrap_or_else(|error| panic!("{alias} must be callable: {error}"));
            assert!(result_text(&value).starts_with("called:"), "{value}");
        }
        // The two long names sanitize to different bases, so both stay distinct.
        assert_ne!(published[0], published[1]);

        manager.shutdown().await;
    }

    /// De-duplication must keep working once aliases are capped, and the
    /// suffix must stay inside the limit.
    #[cfg(windows)]
    #[tokio::test]
    async fn mcp_alias_dedup_survives_the_length_cap() {
        // Three tools whose names all sanitize to the same base.
        let script = r#"
const rl = require('readline').createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const m = JSON.parse(line);
  const send = (result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }) + '\n');
  if (m.method === 'initialize') send({ protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'mock', version: '1' } });
  if (m.method === 'tools/list') send({ tools: [
    { name: 'same name', description: 'one', inputSchema: { type: 'object', properties: {} } },
    { name: 'same-name', description: 'two', inputSchema: { type: 'object', properties: {} } },
    { name: 'same_name', description: 'three', inputSchema: { type: 'object', properties: {} } }
  ]});
  if (m.method === 'tools/call') send({ content: [{ type: 'text', text: 'called:' + m.params.name }] });
});
"#;
        let config = McpServerConfig {
            id: "srv".into(),
            name: "Dedup MCP".into(),
            command: "node".into(),
            args: vec!["-e".into(), script.into()],
            env: HashMap::new(),
            enabled: true,
        };

        let manager = McpManager::connect(&[config], &audit_cwd()).await;
        let published: Vec<String> = manager.tools.iter().map(published_alias).collect();
        assert_eq!(
            published,
            vec![
                "mcp__srv__same_name",
                "mcp__srv__same_name_2",
                "mcp__srv__same_name_3"
            ]
        );
        for alias in &published {
            assert!(alias.chars().count() <= 64, "{alias} is too long");
            assert!(manager.contains(alias));
            let value = manager.call(alias, serde_json::json!({})).await.unwrap();
            assert!(result_text(&value).starts_with("called:"), "{value}");
        }
        manager.shutdown().await;
    }

    /// Short aliases must be left exactly as they were.
    #[test]
    fn short_mcp_aliases_are_not_reshaped() {
        assert_eq!(build_alias("mock", "echo", None), "mcp__mock__echo");
        assert_eq!(
            build_alias("My Server!", "Echo Text", None),
            "mcp__my_server__echo_text"
        );
        assert_eq!(build_alias("mock", "echo", Some(2)), "mcp__mock__echo_2");
    }

    /// Every de-duplication suffix must yield a distinct, in-limit alias, so the
    /// `while calls.contains_key(...)` loop can never spin forever.
    #[test]
    fn capped_mcp_aliases_stay_unique_for_every_dedup_suffix() {
        let long_id = "a".repeat(80);
        let long_name = "b".repeat(80);
        let base = build_alias(&long_id, &long_name, None);
        assert_eq!(base.chars().count(), MAX_ALIAS_CHARS);

        let mut seen = std::collections::HashSet::new();
        seen.insert(base);
        for suffix in 2..=2000u32 {
            let alias = build_alias(&long_id, &long_name, Some(suffix));
            assert!(
                alias.chars().count() <= MAX_ALIAS_CHARS,
                "suffix {suffix} produced a {}-char alias",
                alias.chars().count()
            );
            assert!(seen.insert(alias), "suffix {suffix} collided with another");
        }
    }
}
