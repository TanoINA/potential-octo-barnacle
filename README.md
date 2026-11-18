# AetherAgent

A desktop AI coding agent for Windows and Linux. You give it a folder on your machine, pick a model, and talk to it in chat. It can read and edit files, run shell commands, search the web, and render HTML previews in a sandbox pane.

The backend is written in Rust using Tauri 2. The frontend is a React single-page app with TypeScript and Tailwind CSS.

## Features

- Local workspace tools: read, patch, create files, grep code, and execute terminal commands.
- Live sandboxed preview: renders HTML and SVG artifacts in an isolated iframe.
- Web search: searches the web and pulls page text directly into context when you need docs or external fixes.
- Context mentions: type `@` in the composer to drop workspace files into context pills.
- Auto-titling: automatically names new chat sessions after the first response.
- Multiple providers: works with Anthropic, OpenAI, Ollama, Groq, OpenRouter, or any OpenAI-compatible API.
- Extensibility: supports Model Context Protocol (MCP) servers, background subagents, and modular skills.
- Local storage: conversations, prompt queues, and compaction summaries stay on your disk.

## Requirements

- Node.js 22.12 or newer
- Rust stable
- [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/) for your operating system

## Quick start

Clone the repo and install dependencies:

```sh
git clone https://github.com/TanoINA/potential-octo-barnacle.git
cd potential-octo-barnacle
npm ci
```

Run the desktop app in development mode:

```sh
npm run tauri -- dev
```

Open Settings, choose an LLM provider (or point to local Ollama), pick a workspace folder, and start chatting.

## Packaging

```sh
# Windows: NSIS setup and MSI installers
npm run tauri -- build --bundles nsis,msi -- --locked

# Linux: DEB, RPM, and AppImage packages
npm run tauri -- build --bundles deb,rpm,appimage -- --locked
```

Binaries are placed in `src-tauri/target/release/`, and installer packages in `src-tauri/target/release/bundle/`. Local builds are unsigned. Details on signing and updates are in [docs/release.md](docs/release.md).

## Development and testing

```sh
# Frontend unit tests
npm test

# Frontend build check
npm run build

# Backend unit tests
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked
```

### Directory layout

- `src/`: React UI, hooks, and test suites
- `src-tauri/`: Rust runtime, Tauri commands, agent loop, tools, and SQLite storage
- `scripts/`: Dev scripts and release checks
- `docs/`: Architecture, conversation flow, and release guides

## Data and privacy

App configuration, keys, and session databases are stored in `~/.aether-agent/`. Workspace attachments stay inside `.aether-agent/` in the opened project folder. None of these folders or `.local/` build files are tracked by Git.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Conversation UX](docs/conversation-ux.md)
- [Release guide](docs/release.md)
- [Documentation index](docs/README.md)
- [Changelog](CHANGELOG.md)

New installations receive lightweight quality and web-development guides, including verification before completion, frontend design, browser testing, accessibility review, and React practices. Existing user skills are preserved. See [bundled skills](docs/bundled-skills.md) for the default set and tooling requirements.
