# AetherAgent

AetherAgent is a native Windows desktop AI coding agent built with **Rust, Tauri 2, React 19, TypeScript, and Tailwind CSS**. Its provider, tool, skill, MCP, session, and streaming architecture is ported from the core workflows in `D:\hermes-agent-main`.

## Features

- OpenAI-compatible, GutsAI, DeepSeek, Anthropic, and local Ollama providers
- Live model discovery, custom models, persistent favorites, thinking controls, and reasoning effort selection
- Streaming answer text, extended reasoning, and tool-call argument deltas
- Workspace-scoped file reading, writing, exact editing with diffs, directory trees, and content search
- Asynchronous PowerShell execution with live terminal artifacts and cancellation
- Web page fetching with bounded responses and HTML-to-text/Markdown extraction
- `SKILL.md` discovery and YAML frontmatter catalog injection
- JSON-RPC 2.0 MCP stdio client support
- Persistent chats, search, automatic titles, rename, delete, JSON/Markdown export, and workspace switching
- Virtualized chat and artifact viewers with a compact artifact sidecar for large outputs
- Slash commands and workspace-file `@` mentions

## Run in development

```powershell
npm install
npm run tauri dev
```

Or use `run-aether.ps1` / `run-dev.bat`.

## Build

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

The main release executable is generated at:

```text
src-tauri/target/release/aether-agent.exe
```

The packaging step also produces native installers under `src-tauri/target/release/bundle/` when the corresponding Windows packaging tools are available. The repository-root `AetherAgent.exe` is a copy of the verified main release executable.

## Data and configuration

AetherAgent stores configuration, workspace state, and session JSON under:

```text
%USERPROFILE%\.aether-agent\
```

Provider credentials are stored in the user-local configuration file. Do not share that file. Tool file access is constrained to the selected workspace; native export and workspace selection require an explicit destination through the operating-system dialog.

## Architecture

```text
React UI → Tauri commands/events → AgentRunner
                                  ├─ Provider adapters + SSE
                                  ├─ Native workspace tools
                                  ├─ Skill loader
                                  ├─ MCP stdio clients
                                  └─ Atomic JSON persistence
```

Large file reads, diffs, and terminal logs are emitted as artifacts rather than expanded into chat bubbles. The interface uses CSS-only transitions and does not require Three.js, canvas particle loops, or other GPU animation frameworks.

## Performance verification

Virtualization bounds the number of mounted chat rows; it does not by itself guarantee a particular WebView2 process-memory figure or frame rate. Measure those on the target machine with Windows Performance Analyzer/Task Manager and a representative long session before making hardware-specific claims.
