# AetherAgent

A desktop coding agent for Windows and Linux, built with Rust, Tauri 2, and React.

Still a work in progress. Expect bugs and rough edges.

It can read and edit files in your workspace, run commands, and connect to MCP servers. Chats support image attachments, skills, and subagents. You can queue prompts while a task is running and switch conversations without losing the queue.

## Run locally

You'll need Node.js 22.12+, Rust stable, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
npm ci
npm run tauri -- dev
```

Choose a provider and model in Settings once the app opens.

## Build

```sh
# Windows
npm run tauri -- build --bundles nsis,msi -- --locked

# Linux
npm run tauri -- build --bundles deb,rpm,appimage -- --locked
```

The binary and installers end up in `src-tauri/target/release/`. Linux compatibility depends on the distro used to build the packages.

Local builds are unsigned unless you configure signing. In-app updates also need an update server and signing keys; they aren't enabled by default. See the [release guide](docs/release.md) for setup.

## Tests

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked
```

## Where things live

- `src/` contains the React app and its tests.
- `src-tauri/` contains the Rust runtime, tools, and storage.
- `scripts/` has development and release scripts.
- [docs/](docs/README.md) has the longer explanations. Old audits are in [docs/archive/](docs/archive/README.md).
- [rune/](rune/README.md) is an experimental native UI, separate from the Tauri app.

Sessions, settings, and skills are stored in `~/.aether-agent/`. Workspace attachments may also use a `.aether-agent/` folder inside the project. These files stay out of Git, as do the local builds and logs under `.local/`.

[Changelog](CHANGELOG.md)
