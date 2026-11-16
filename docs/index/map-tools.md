# Module Guide: `src-tauri/src/agent/tools.rs`

The `tools` module defines and executes all native operating-system capabilities available to the model during an agent run.

## Core Responsibilities

1. **Tool Definitions (`get_agent_tools`)**:
   - Publishes OpenAI/Anthropic-compatible JSON schemas for all native tools (`read_file`, `edit_file`, `write_file`, `list_directory`, `search_files`, `run_command`, `fetch_web_page`, `ask_question`, etc.).
2. **Filesystem Tools & Safety Boundary**:
   - `read_file`, `write_file`, `edit_file`: Reads line ranges, applies exact search-and-replace patches, and manages atomic file creation.
   - `resolve_workspace_path`: Validates that requested paths remain bounded within the active workspace root, rejecting directory traversal (`..`), device paths, and symlink escapes.
3. **Command Execution (`run_command`)**:
   - Executes shell commands in the background or synchronously with configurable timeouts.
   - Streams terminal output deltas via `terminal` events and integrates with `tasks.rs` for long-running process lifecycle management.
4. **Web Content Retrieval (`fetch_web_page`)**:
   - Validates target URLs (`validate_public_url`) and prevents internal network probing (`is_public_ip`).
   - Extracts clean page text and converts HTML structures into Markdown for model consumption.
5. **Interactive Tools (`ask_question`)**:
   - Emits structured clarification prompts to the chat UI and blocks execution until the user selects or inputs an answer.

## Key Types and Functions

- `ToolExecutionResult`: Returns standard execution status, output payload, and artifact metadata.
- `execute_tool`: Main router that receives a tool call name and arguments, executes the corresponding handler, and formats the output.
- `resolve_workspace_path`: Security boundary ensuring file operations stay inside the active workspace.
