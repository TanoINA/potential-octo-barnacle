# Aether Rune: experimental native desktop host

Aether Rune is an experimental native desktop host for AetherAgent written in pure Rust using winit and wgpu. Instead of running a React frontend inside a Tauri WebView, Rune renders its UI directly on the GPU.

The production reference client remains the Tauri app in `src/` and `src-tauri/`. Rune is currently a standalone companion workspace under active development: the core chat interface, state machine, modal panels, and export work, but it does not yet have full feature parity with the Tauri version.

For a detailed breakdown of component parity, missing features, and IPC mappings, see [docs/feature-parity.md](docs/feature-parity.md).

## Build and run

Prerequisites:
- A Rust toolchain supporting edition 2024 and pinned LaTeX dependencies (Rust 1.99+)
- C++ build tools (MSVC and Windows SDK on Windows)
- A working graphics driver supporting Vulkan, DirectX 12, or Metal

Run Cargo from inside the `rune/` directory so it picks up `rune/.cargo/config.toml`:

```powershell
cd rune

# Run with the default development profile
cargo run --locked -p aether-rune

# Or run with a custom isolated profile
cargo run --locked -p aether-rune -- --profile "$env:LOCALAPPDATA\AetherRune\development"

# Run a quick startup smoke test (initializes state in a temp profile and exits)
cargo run --locked -p aether-rune -- --smoke
```

You can also launch it from the repository root:

```powershell
cargo run --manifest-path rune/Cargo.toml --locked -p aether-rune -- --profile "$env:LOCALAPPDATA\AetherRune\development"
```

## Current capabilities

Rune includes native implementations for primary workflows:

- **Chat**: streaming text, reasoning blocks, tool call presentations, and model candidate switching.
- **Session management**: session list, history browsing, timeline controls, message edit/resend, retry, undo, and rollback.
- **Attachments**: file and image pickers via native OS dialogs, clipboard paste, drop handling, and decoded image previews.
- **Settings**: model selection, provider configuration, MCP and tool management, workspace memory, and task execution.
- **Export**: native export to Markdown, HTML, and JSON with save dialogs and cancellation handling.
- **Commands**: composer slash commands including `/new`, `/clear`, `/undo`, `/retry`, `/title`, `/stop`, `/compact`, `/review`, `/plan`, and `/export`.

## Safe profile isolation

To prevent accidental corruption of your main AetherAgent sessions and settings, Rune uses strict profile separation:

- Default profile directory: `~/.aether-agent-rune` (completely separate from production `~/.aether-agent`).
- Custom profile flag: `--profile ABSOLUTE_PATH`.
- Import flag: `--import-profile ABSOLUTE_SOURCE`.

Startup verifies and canonicalizes the profile path before creating the backend or loading settings. Traversal components, symlinks, junction points, and Windows trailing-dot paths are rejected. Profile switching requires restarting the application.

### Importing from an existing profile

If you want to test Rune with your existing chat history, you can explicitly import data into a fresh profile:

```powershell
cargo run --locked -p aether-rune -- --profile "$env:LOCALAPPDATA\AetherRune\imported-copy" --import-profile "$env:USERPROFILE\.aether-agent"
```

The import process:
- Copies only `config.json`, `workspace.json`, `sessions/*.json`, and valid `sessions/*.checkpoint` recovery sidecars.
- Validates file integrity, checksums, and timestamps before writing.
- Never copies running tasks, vectors, raw logs, or SQLite indices (`session-pages.sqlite` rebuilds automatically).
- Refuses to write into a directory that already contains files.

## Known feature gaps

Key differences from the Tauri baseline:

- **Composer UX**: no `@` file mention autocomplete popup or Spotlight command palette.
- **Context menus**: basic text copy and message actions are available, but code-action helpers (quote, explain, refactor) are not yet ported.
- **Interactive plans**: fenced plan blocks render as code rather than interactive multi-state progress checklists.
- **Math rendering**: inline and display LaTeX render via `latex-rust` SVG paths, covering common math formulas, but full KaTeX feature parity is not complete.
- **OS integrations**: global `Alt+Space` shortcut, audio chime, and system notifications are not fully wired.
- **Automatic updates**: Rune does not include signed updater support or auto-update binaries.

## Testing

Run tests from the `rune/` directory:

```powershell
# Run all workspace unit tests
cargo test --workspace --locked

# Profile isolation tests
cargo test --locked -p aether-core --lib agent::profile

# Linting and formatting checks
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```
