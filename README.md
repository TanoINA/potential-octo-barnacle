# AetherAgent

AetherAgent is a desktop coding agent for Windows and Linux built with Tauri 2, Rust, and React. You point it at a local project folder, pick an LLM provider, and work with the codebase from chat. The Rust backend runs file tools, shell commands, SQLite search and history storage, and MCP servers, while React drives the UI.

## What it can do

- Work inside a local workspace: read, patch, create, and grep files, or run commands in an integrated shell.
- Dock and resize the side pane on desktop to read code, inspect diffs, or run commands side by side, and preview HTML and SVG artifacts in sandboxed iframes isolated from the app window.
- Switch between five built-in dark and light themes from Settings, the sidebar, or the bottom status bar.
- Pull web search results and page text into the conversation, and use `@` mentions in the composer to attach workspace files or folders.
- Talk to Anthropic, OpenAI, Ollama, Groq, OpenRouter, or custom OpenAI-compatible endpoints, with session auto-titling, prompt queues, and context compaction.
- Add extra capabilities through stdio MCP servers, background subagents, and local `SKILL.md` skills. Starter guides seed into `~/.aether-agent/skills/` on first launch without overwriting anything you already have there.

## Requirements

- Node.js 22.12 or newer
- Stable Rust toolchain
- [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/) for Windows or Linux

## Quick start

Clone the repo, install packages, and start the dev build:

```sh
git clone https://github.com/TanoINA/potential-octo-barnacle.git
cd potential-octo-barnacle
npm ci
npm run tauri -- dev
```

Once the window opens, go to Settings to pick a provider (or point at a local Ollama server), choose a workspace folder, and start a chat.

## Packaging

```sh
# Windows: NSIS setup and MSI installers
npm run tauri -- build --bundles nsis,msi -- --locked

# Linux: DEB, RPM, and AppImage packages
npm run tauri -- build --bundles deb,rpm,appimage -- --locked
```

Compiled binaries land in `src-tauri/target/release/`, and installers go to `src-tauri/target/release/bundle/`. Local builds are unsigned; see [docs/release.md](docs/release.md) for signing and auto-updater setup.

## Development and testing

```sh
# Frontend unit tests and production build
npm test
npm run build

# Backend Rust tests (on Windows, add -- --test-threads=1 if spawned shell tests time out under parallel load)
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked
```

Where things live:

- `src/`: React UI, hooks, and Vitest specs
- `src-tauri/`: Rust backend, Tauri commands, agent loop, tools, and SQLite storage
- `scripts/`: release policy checks and packaging verification scripts
- `docs/`: architecture, UX, bundled skills, and release guides
- `.aether/skills/`: source Markdown for starter skills bundled into the binary

## Data and privacy

Settings, provider credentials, global skills, and session databases stay on your machine in `~/.aether-agent/`. Workspace attachments stay in `.aether-agent/` inside the opened project. Git ignores both of those along with `.local/` (used for local test logs, fixture snapshots, and retained installers).

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Bundled skills](docs/bundled-skills.md)
- [Conversation UX](docs/conversation-ux.md)
- [Documentation index](docs/README.md)
- [Release guide](docs/release.md)
- [Changelog](CHANGELOG.md)
