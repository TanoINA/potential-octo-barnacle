# Backend Files (`src-tauri/src/`)

This index lists the Rust backend modules powering AetherAgent.

## Core Application & Entry Points

- **`main.rs`**: Native binary entry point.
- **`lib.rs`**: Tauri application builder, IPC command registration, and global state wiring.
- **`agent/mod.rs`**: Module re-exports and process spawning utilities.
- **`agent/types.rs`**: Shared data structures, provider settings, and execution options.
- **`agent/events.rs`**: Event emission channels to the Tauri frontend.

## Agent Execution & Providers

- **`agent/runner.rs`**: Main agent loop, turn orchestration, prompt assembly, and subagent management.
- **`agent/provider.rs`**: LLM API adapters for OpenAI-compatible and Anthropic endpoints.
- **`agent/tools.rs`**: Tool definitions and execution handlers for workspace operations.
- **`agent/tasks.rs`**: Background task tracking, stdout/stderr streaming, and process termination.
- **`agent/pty.rs`**: Terminal command execution and Windows console support.

## Storage, Persistence & History

- **`agent/storage.rs`**: Canonical JSON session persistence, settings management, and exports.
- **`agent/session_pages.rs`**: Bounded history paging, lazy offset reading, and SQLite projections.
- **`agent/session_journal.rs`**: Append-only transactional logging and delta recovery.
- **`agent/search.rs`**: SQLite full-text search indexing across chat histories.
- **`agent/profile.rs`**: User profile isolation, directory validation, and data migration.

## Integrations & Intelligence

- **`agent/mcp.rs`**: Model Context Protocol (MCP) server lifecycle and tool execution.
- **`agent/skills.rs`**: Skill discovery, system prompt normalization, and local catalog rendering.
- **`agent/reflection.rs` & `agent/review.rs`**: Post-turn autonomous skill review loop.
- **`agent/review_skills.rs` & `agent/review_pending.rs`**: Skill patch execution, staging, and approval diffs.
- **`agent/images.rs`**: Multimodal image attachment validation and base64 payload formatting.
- **`agent/git.rs`**: Git repository status, file staging, and commit operations.
- **`agent/release.rs` & `agent/release_policy.rs`**: In-app update checking, download limits, and verification.
