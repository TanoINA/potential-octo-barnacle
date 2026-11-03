use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;
use uuid::Uuid;
use super::types::Artifact;

pub fn get_agent_tools() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read contents of a file. For large files, specify start_line and end_line.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Absolute or relative file path" },
                        "start_line": { "type": "number", "description": "Start line (1-indexed)" },
                        "end_line": { "type": "number", "description": "End line (inclusive)" }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Create or overwrite a file with given content.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Absolute or relative file path" },
                        "content": { "type": "string", "description": "Full content to write" }
                    },
                    "required": ["path", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "List files and folders in a directory.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Directory path (defaults to current workspace)" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Execute a PowerShell command in the workspace.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "The command line string to run" }
                    },
                    "required": ["command"]
                }
            }
        }
    ])
}

pub struct ToolExecutionResult {
    pub output: String,
    pub artifact: Option<Artifact>,
}

pub async fn execute_tool(
    name: &str,
    args: &serde_json::Value,
    cwd: &str,
) -> ToolExecutionResult {
    match name {
        "read_file" => {
            let rel_path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let target_path = resolve_path(cwd, rel_path);

            match fs::read_to_string(&target_path).await {
                Ok(content) => {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = args.get("start_line").and_then(|v| v.as_u64()).map(|v| (v.saturating_sub(1)) as usize).unwrap_or(0);
                    let end = args.get("end_line").and_then(|v| v.as_u64()).map(|v| (v as usize).min(lines.len())).unwrap_or(lines.len());

                    let slice = if start < lines.len() {
                        lines[start..end.min(lines.len())].join("\n")
                    } else {
                        content.clone()
                    };

                    if lines.len() > 25 {
                        let art_id = format!("art-{}", Uuid::new_v4());
                        let ext = target_path.extension().and_then(|e| e.to_str()).unwrap_or("txt").to_string();
                        let filename = target_path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();

                        let artifact = Artifact {
                            id: art_id.clone(),
                            title: filename.clone(),
                            artifact_type: "code".to_string(),
                            language: Some(ext),
                            content: slice.clone(),
                            file_path: Some(target_path.to_string_lossy().to_string()),
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        };

                        let preview = lines.iter().take(12).copied().collect::<Vec<_>>().join("\n");
                        ToolExecutionResult {
                            output: format!("[Read {} lines from {} (Opened in Artifact Sidecar)]\nPreview:\n{}...", lines.len(), filename, preview),
                            artifact: Some(artifact),
                        }
                    } else {
                        ToolExecutionResult {
                            output: slice,
                            artifact: None,
                        }
                    }
                }
                Err(e) => ToolExecutionResult {
                    output: format!("Failed to read file: {}", e),
                    artifact: None,
                },
            }
        }

        "write_file" => {
            let rel_path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let target_path = resolve_path(cwd, rel_path);

            if let Some(parent) = target_path.parent() {
                let _ = fs::create_dir_all(parent).await;
            }

            match fs::write(&target_path, content).await {
                Ok(_) => {
                    let art_id = format!("art-{}", Uuid::new_v4());
                    let ext = target_path.extension().and_then(|e| e.to_str()).unwrap_or("txt").to_string();
                    let filename = target_path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();

                    let artifact = Artifact {
                        id: art_id,
                        title: format!("Saved: {}", filename),
                        artifact_type: "code".to_string(),
                        language: Some(ext),
                        content: content.to_string(),
                        file_path: Some(target_path.to_string_lossy().to_string()),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    };

                    ToolExecutionResult {
                        output: format!("Successfully wrote {} bytes to {}", content.len(), target_path.to_string_lossy()),
                        artifact: Some(artifact),
                    }
                }
                Err(e) => ToolExecutionResult {
                    output: format!("Failed to write file: {}", e),
                    artifact: None,
                },
            }
        }

        "list_directory" => {
            let rel_path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let target_path = if rel_path.is_empty() {
                PathBuf::from(cwd)
            } else {
                resolve_path(cwd, rel_path)
            };

            match fs::read_dir(&target_path).await {
                Ok(mut entries) => {
                    let mut items = Vec::new();
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                        items.push(format!("{}{}", if is_dir { "[DIR] " } else { "      " }, name));
                    }
                    ToolExecutionResult {
                        output: if items.is_empty() { "(empty directory)".to_string() } else { items.join("\n") },
                        artifact: None,
                    }
                }
                Err(e) => ToolExecutionResult {
                    output: format!("Failed to list directory: {}", e),
                    artifact: None,
                },
            }
        }

        "run_command" => {
            let cmd_str = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let output = Command::new("powershell.exe")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(cmd_str)
                .current_dir(cwd)
                .output()
                .await;

            match output {
                Ok(res) => {
                    let stdout = String::from_utf8_lossy(&res.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&res.stderr).to_string();
                    let combined = format!("{}{}", stdout, if !stderr.is_empty() { format!("\n[STDERR]\n{}", stderr) } else { "".to_string() });

                    let lines: Vec<&str> = combined.lines().collect();
                    if lines.len() > 20 {
                        let art_id = format!("art-{}", Uuid::new_v4());
                        let artifact = Artifact {
                            id: art_id,
                            title: format!("Terminal: {}...", &cmd_str[..cmd_str.len().min(25)]),
                            artifact_type: "terminal".to_string(),
                            language: Some("sh".to_string()),
                            content: combined.clone(),
                            file_path: None,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        };

                        let preview = lines.iter().take(10).copied().collect::<Vec<_>>().join("\n");
                        ToolExecutionResult {
                            output: format!("[Command finished ({} lines) -> Full output in Artifact Panel]\nPreview:\n{}...", lines.len(), preview),
                            artifact: Some(artifact),
                        }
                    } else {
                        ToolExecutionResult {
                            output: if combined.is_empty() { "(command finished with no output)".to_string() } else { combined },
                            artifact: None,
                        }
                    }
                }
                Err(e) => ToolExecutionResult {
                    output: format!("Failed to execute command: {}", e),
                    artifact: None,
                },
            }
        }

        _ => ToolExecutionResult {
            output: format!("Unknown tool: {}", name),
            artifact: None,
        },
    }
}

fn resolve_path(cwd: &str, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(cwd).join(path)
    }
}
