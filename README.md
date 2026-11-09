# AetherAgent

A desktop AI coding assistant built with **Tauri 2, Rust, and React**. Windows and Linux builds are supported; the project is under active development.

## Features

- Streaming conversations with workspace tools, image attachments, artifacts, and MCP.
- Searchable skill selection with per-message context, not prompt templates.
- Per-chat prompt queues with send-now, edit, and remove actions.
- Sidebar run status and compact activity details grouped by subagent.
- Paged, virtualized conversation history with guarded saves and recovery.
- Signed-update infrastructure; automatic updates stay disabled until the operator configures a trusted endpoint and signing identity.

## Development

Install Node.js 22.12+ and Rust stable, plus the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
npm ci
npm run tauri -- dev
```

## Checks and builds

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked

# Windows: EXE installer and MSI
npm run tauri -- build --bundles nsis,msi -- --locked

# Linux: use the documented GTK/WebKit prerequisites
npm run tauri -- build --bundles deb,rpm,appimage -- --locked
```

Build output is generated under `src-tauri/target/release/`. Installers and test logs are **not source files** and are not committed. Retain local packages under `.local/releases/`; use [GitHub Releases](https://github.com/TanoINA/potential-octo-barnacle/releases) for published packages. Unsigned local builds are not trusted updater releases.

## Repository map

| Path | Purpose |
| --- | --- |
| [`src/`](src/) | React UI and frontend tests |
| [`src-tauri/`](src-tauri/) | Native runtime, tools, persistence, and Rust tests |
| [`scripts/`](scripts/) | Development and release utilities |
| [`docs/`](docs/README.md) | Architecture, usage notes, and release procedures |
| [`rune/`](rune/README.md) | Separate experimental native UI; not the shipped Tauri app |
| `.local/` | Ignored local packages, verification evidence, and maintenance backups |

Root configuration files are kept where the build tools expect them. Historical audits live in [`docs/archive/`](docs/archive/README.md), not alongside current guides.

## Local data and safety

User sessions, settings, and skills live in `~/.aether-agent/` (`%USERPROFILE%\.aether-agent\` on Windows). Workspace attachments may use `.aether-agent/` inside the project. These directories, credentials, signing keys, and local artifacts are excluded from Git.

See [documentation](docs/README.md), [release procedures](docs/release.md), and the [changelog](CHANGELOG.md). A local compile or passing fixture test does not certify a signed upgrade or compatibility with every Linux distribution.
