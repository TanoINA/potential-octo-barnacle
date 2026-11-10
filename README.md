# AetherAgent

A desktop coding agent for Windows and Linux. Work with an AI model on a local codebase: read and edit files, run commands, and review the results in chat.

Built with Rust, Tauri 2, React, and TypeScript. Still in development.

## Features

- File editing, code search, and command execution in your workspace
- Image attachments, skills, MCP servers, and subagents
- Multiple chats with prompt queues and per-chat run status
- Saved conversations and context compaction

## Getting started

Install Node.js 22.12+, Rust stable, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
git clone https://github.com/TanoINA/potential-octo-barnacle.git
cd potential-octo-barnacle
npm ci
npm run tauri -- dev
```

Open Settings to configure your model provider, then choose a workspace.

## Build

```sh
# Windows: EXE and MSI installers
npm run tauri -- build --bundles nsis,msi -- --locked

# Linux: DEB, RPM, and AppImage packages
npm run tauri -- build --bundles deb,rpm,appimage -- --locked
```

Build on the target OS. The executable is in `src-tauri/target/release/`; installers are under `src-tauri/target/release/bundle/`. Linux compatibility depends on the build distro.

Local builds are unsigned by default. Signing and in-app update setup are covered in the [release guide](docs/release.md).

## Development

- `src/` — React UI and frontend tests
- `src-tauri/` — Rust agent runtime, tools, and storage
- `scripts/` — development and release scripts
- `rune/` — experimental native UI, separate from the Tauri app

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked
```

Local app data lives in `~/.aether-agent/`. Attachments can also use `.aether-agent/` inside a workspace. These folders and the build archives in `.local/` are excluded from Git.

## Documentation

[Architecture](docs/ARCHITECTURE.md) · [Chat and workflow](docs/conversation-ux.md) · [Releases](docs/release.md) · [All docs](docs/README.md) · [Changelog](CHANGELOG.md)
