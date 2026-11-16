# Module Guide: `src-tauri/src/agent/storage.rs`

The `storage` module manages on-disk persistence for AetherAgent, including configuration files, workspace selections, session histories, artifact files, and checkpoint recovery records.

## Core Responsibilities

1. **Storage Profile Management (`config_dir`)**:
   - Manages the user data root (defaults to `~/.aether-agent/`).
   - Stores global preferences (`config.json`), active workspace pointers (`workspace.json`), and individual session files (`sessions/`).
2. **Atomic Writes (`atomic_write`)**:
   - Writes new content to temporary files before replacing target files in a single atomic rename.
   - Prevents file corruption during unexpected power loss or process crashes.
3. **Session Lifecycle and History**:
   - `save_session` / `load_session`: Full canonical JSON serialization and deserialization.
   - `save_session_page` / `load_session_page`: Windowed reads and partial saves for lazy history pagination.
   - `replace_session_checked`: CAS (Compare-And-Swap) protection against concurrent or stale writes.
4. **Settings and Migrations**:
   - `load_settings` / `save_settings`: Loads typed `AgentSettings`, fills in defaults, and migrates legacy provider configurations.
5. **Artifacts and Exports**:
   - Stores tool output artifacts (diffs, web text, command outputs).
   - Generates standalone exports to Markdown, JSON, and self-contained HTML (`render_standalone_html`).

## Key Types and Functions

- `AgentSettings`: Struct containing provider configs, thinking effort, reflection parameters, and UI preferences.
- `CommittedSaveProof`: Cryptographic verification token returned after a successful disk write.
- `list_sessions`: Reads session metadata (title, updated timestamp, message counts) for sidebar rendering.
- `checkpoint_session`: Writes fast recovery checkpoints during active streaming runs.
