# Rune Files (`rune/`)

This index lists the Rust crates and modules composing the experimental native GPU-rendered desktop host.

## Workspace Architecture

- **`rune/Cargo.toml`**: Cargo workspace manifest for Rune crates.
- **`rune/docs/feature-parity.md`**: Feature parity tracking comparing Rune against the React/Tauri baseline.
- **`rune/README.md`**: Rune build guide, runtime requirements, and profile isolation details.

## Crate: `aether-core` (`rune/crates/aether-core/`)

Shared backend service layer:
- **`service.rs`**: Backend command dispatcher (`Backend::dispatch`) handling IPC-like operations in-process.
- **`updater.rs`**: Native updater interface for checking and staging updates.
- **`agent.rs`**: Core agent traits and command creation helpers.

## Crate: `aether-state` (`rune/crates/aether-state/`)

Renderer-independent state controller:
- **`controller.rs`**: Main controller holding the Tokio runtime, backend reference, and public model state.
- **`history.rs`**: Session navigation, turn editing, undo, retry, and rollback logic.
- **`reducer.rs` & `events.rs`**: Event ingestion loop updating local model state from backend streams.
- **`commands.rs`**: Controller action dispatching and command result caching.

## Crate: `aether-rune` (`rune/crates/aether-rune/`)

Native GPU-rendered UI using winit and wgpu:
- **`main.rs` & `app.rs`**: Application entry point, window management, and frame loop.
- **`cli.rs`**: CLI flag parsing (`--profile`, `--import-profile`, `--smoke`).
- **`view.rs` & `widgets.rs`**: Chat layout, message list rendering, and UI building blocks.
- **`panels.rs`**: Modal dialog panels (Settings, Models, MCP, Tools, Git, Export).
- **`rich_content.rs`**: Markdown parsing and native `latex-rust` SVG formula rendering.
- **`composer.rs` & `completion.rs`**: Text composer and slash command auto-completion.
- **`statequeue.rs`**: In-memory prompt queue management.
- **`native.rs`**: Native OS dialogs (file picker, folder picker), clipboard paste, and image attachment preview.
- **`desktop.rs`**: System tray and desktop shortcut handling.
- **`export.rs`**: Native export to Markdown, HTML, and JSON.
