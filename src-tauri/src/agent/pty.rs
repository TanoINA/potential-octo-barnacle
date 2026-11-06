use serde::{Deserialize, Serialize};
use std::time::Instant;

use super::tools::{execute_tool, ToolContext};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellCommandResult {
    pub stdout: String,
    pub stderr: String,
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
}

/// Executes one bounded, non-interactive workspace shell command.
/// Output is combined in terminal order and capped by the shared command runner.
pub async fn run_shell_command(cwd: &str, command_str: &str) -> Result<ShellCommandResult, String> {
    let command = command_str.trim();
    if command.is_empty() {
        return Err("Command cannot be empty".into());
    }
    let started = Instant::now();
    let context = ToolContext {
        cwd: cwd.to_string(),
        cancel: CancellationToken::new(),
        sink: None,
    };
    let result = execute_tool(
        "run_command",
        &serde_json::json!({"command": command, "timeoutSeconds": 120}),
        &context,
    )
    .await;
    let output = result
        .terminal_output
        .clone()
        .unwrap_or_else(|| result.output.clone());

    Ok(ShellCommandResult {
        stdout: output,
        stderr: String::new(),
        exit_code: result
            .exit_code
            .unwrap_or(if result.success { 0 } else { 1 }),
        duration_ms: started.elapsed().as_millis() as u64,
    })
}
