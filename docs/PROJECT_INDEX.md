# Codebase Index and Architecture Map

A guide to the structure, subsystems, and module boundaries across the AetherAgent repository.

## Subsystem breakdown

| Area | Description | Primary Location | Index |
| --- | --- | --- | --- |
| **Backend** | Native Rust runtime, Tauri commands, agent loop, tools, SQLite storage, and MCP management. | `src-tauri/src/agent/` | [files-backend.md](index/files-backend.md) |
| **Frontend** | React single-page app, virtualized chat list, composer, settings modal, and hooks. | `src/` | [files-frontend.md](index/files-frontend.md) |
| **Rune** | Experimental native GPU-rendered desktop interface written in pure Rust (winit/wgpu). | `rune/` | [files-rune.md](index/files-rune.md) |
| **Release & CI** | Packaging scripts, Authenticode and updater verifiers, and GitHub Actions workflows. | `scripts/`, `.github/` | [files-release.md](index/files-release.md) |
| **Documentation** | Architectural specifications, UX guides, and release procedures. | `docs/` | [files-docs.md](index/files-docs.md) |
| **Configuration** | Root project manifests, Vite, Tailwind, TypeScript, and Cargo workspace configs. | Root directory | [files-config.md](index/files-config.md) |

## Core Backend Module Maps

Detailed architecture and function guides for primary backend components:

- **[Runner (runner.rs)](index/map-runner.md)**: Main agent execution loop, prompt hydration, tool dispatch, and subagent orchestration.
- **[Provider (provider.rs)](index/map-provider.md)**: Adapter translations for Anthropic and OpenAI-compatible endpoints, SSE streaming, and model discovery.
- **[Storage (storage.rs)](index/map-storage.md)**: Session history persistence, atomic saves, checkpoint sidecars, and profile settings.
- **[Session Journal (session_journal.rs)](index/map-session_journal.md)**: Append-only transaction logging, delta tracking, and crash recovery.
- **[Tools (tools.rs)](index/map-tools.md)**: File reading/writing, shell command execution, workspace resolution, and web search integration.

## Inter-process communication

The frontend communicates with the Tauri core via asynchronous IPC commands and listens for streamed events:

### Primary Event Topics

- `agent-event`: Streamed token deltas, reasoning chunks, and tool execution status updates.
- `agent:micro-compaction`: Notification when background context truncation occurs for long turns.
- `agent:self-improvement`: Post-turn reflection proposals, skill creations, or updates.
- `agent:self-improvement-error`: Failure notifications during background reflection passes.
- `release:update-progress`: Download and verification progress during application updates.
- `terminal`: Streaming stdout and stderr output from background shell tasks.
- `toggle-spotlight`: Global shortcut signal to summon or focus the quick command bar.
