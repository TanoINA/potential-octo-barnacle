# AetherAgent — Full Core Port Report

**Version:** 0.2.0  
**Target:** `D:\AetherAgent`  
**Reference:** `D:\hermes-agent-main`  
**Stack:** Rust, Tauri 2, React 19, TypeScript, Tailwind CSS, Tokio, TanStack Virtual

## Delivery status

The requested provider, agent-tool, skills/MCP, persistence, workspace, model-picker, virtualized-chat, artifact-sidecar, and composer modules have been implemented as native AetherAgent subsystems. The final custom-protocol release and both Windows installer formats have been built, copied, and hash-verified.

## 1. Provider, model, and reasoning engine

### Frontend

`src/components/ModelPicker.tsx` implements:

- Instant `Search models` filtering across model IDs and provider names.
- Provider filtering and provider/model grouping metadata.
- Persistent favorite stars backed by saved `AgentSettings`.
- Inferred `MAX`, `HIGH`, and `PREVIEW` badges.
- Thinking ON/OFF switch with an accessible name and `aria-checked` state.
- Effort options: Minimal, Low, Medium, High, Extra High, Max, Ultra.
- Per-session provider/model/thinking/effort overrides.
- `Use Settings default`, which clears the session override.
- `Refresh models`, backed by native provider discovery.
- `Add custom model...`, persisted under the selected provider.

### Rust backend

`src-tauri/src/agent/provider.rs` implements:

- OpenAI-compatible endpoints, including GutsAI and custom endpoints.
- Anthropic Messages API request/history/tool conversion.
- DeepSeek and Ollama compatibility modes.
- Ollama `/api/tags` fallback in addition to `/v1/models`.
- `thinking`, `reasoning_effort`, temperature, token limits, tool definitions, and provider-specific body extensions.
- Anthropic `x-api-key` / version headers and OpenAI bearer authentication.
- OpenAI SSE text, reasoning, and function-call deltas.
- Anthropic text, thinking, and input-JSON deltas.
- Chunk-boundary-safe byte buffering, CRLF handling, multiline SSE data, `[DONE]`, and non-SSE JSON fallback.
- Ordered accumulation of streamed tool-call arguments.
- Cancellation tokens and provider-error propagation without API-key logging.

## 2. Native agent and tools

`src-tauri/src/agent/tools.rs` and `runner.rs` implement a multi-turn native tool loop with scoped events and cancellation.

| Tool | Ported behavior |
| --- | --- |
| `read_file` | UTF-8 text reads, `startLine`/`endLine` plus snake-case aliases, empty-file handling, binary/size guards, long-read artifacts |
| `write_file` | Automatic parent directories, bounded content, atomic replacement, code artifact |
| `edit_file` | Exact replacement, unique-match enforcement, optional replace-all, unified diff artifact |
| `list_directory` | Sorted directory/tree output, recursion/depth/entry limits |
| `search_files` | Regex content search, optional glob, case control, binary/size guards, result caps |
| `run_command` | Asynchronous PowerShell, stdout/stderr streaming, bounded terminal artifact, timeout, abort, Windows process-tree termination |
| `fetch_web_page` | HTTP(S)-only URL validation, credential/private-address rejection, manual validated redirects, time/size limits, HTML-to-text/Markdown-style extraction |

Additional execution properties:

- Workspace containment is enforced again at the native action boundary.
- Relative traversal, NT device namespaces, reserved Windows names, and symlink escapes are rejected.
- Model-facing output is bounded; large UI output is emitted as artifacts instead of raw chat bubbles.
- Events contain `runId` and `sessionId`, preventing stale updates after session changes.
- Active runs are registered per session and cancellation is scoped by run ID.
- Provider API keys are redacted from propagated provider errors.

## 3. Skills and MCP

### Skill engine

`src-tauri/src/agent/skills.rs` provides:

- Recursive `SKILL.md` discovery.
- YAML frontmatter parsing for `name` and `description`.
- Default roots for the Gemini configuration directory and Hermes source skills directory.
- Additional operator-configured roots.
- Size/depth/catalog limits, deduplication, and compact system-prompt injection.

### MCP stdio

`src-tauri/src/agent/mcp.rs` provides a real JSON-RPC 2.0 client:

- Child-process stdio transport.
- `initialize` and `notifications/initialized`.
- `tools/list` and `tools/call`.
- Request IDs, response correlation, timeouts, malformed-message tolerance, and closed-channel errors.
- Dynamic provider-safe aliases for MCP tools in the agent catalog.
- Direct Tauri discovery/call commands and child-process cleanup.

## 4. Sessions, workspace, and persistence

`src-tauri/src/agent/storage.rs` implements atomic JSON persistence under `%USERPROFILE%\.aether-agent\`:

- Version-tolerant settings with migration of early provider shapes.
- Current nested session schema plus migration of early flat session files.
- Preserved creation timestamps and refreshed update timestamps/message counts.
- Automatic title generation from the first prompt.
- Compact latest-message previews for conversation search.
- Rename, delete, load/save, and safe session-ID validation.
- Native-dialog JSON and Markdown export.
- Persisted active workspace and native directory switching.

`src/components/Sidebar.tsx` exposes new/select/search/rename/delete/export and workspace actions.

## 5. UI and rendering architecture

### Virtualization and streaming

- `ChatCanvas.tsx` uses `@tanstack/react-virtual` with dynamic row measurement and minimal overscan.
- `ArtifactPane.tsx` virtualizes large line collections.
- `useAgent.ts` coalesces text/reasoning/terminal network deltas into animation-frame commits.
- Message rows and the sidebar use memoization; stale run events are ignored.
- No Three.js, particle canvas, or continuous GPU animation loop is used.

### Artifact sidecar

- Explicit **Code Viewer**, **Diff Viewer**, and **Terminal Output** tabs.
- Artifact selection within each viewer type.
- Line numbers, copy action, file/source metadata, and line counts.
- Green/red diff highlighting and live terminal append behavior.
- Long file/search/tree/web/terminal output stays out of chat bubbles.

### Smart composer

- Slash popup: `/clear`, `/model`, `/settings`, `/new`.
- Keyboard navigation, Escape dismissal, Enter/Shift+Enter behavior.
- `@` workspace-file autocomplete backed by native file discovery.
- VCS metadata and generated dependency/build directories are excluded from context suggestions.
- Normal footer layout keeps helper text and send/stop controls clear of the input border.

### Visual system

The interface uses the requested dark palette: `#0d1117`, `#161b22`, `#30363d`, `#e6edf3`, and `#58a6ff`, with visible focus states and reduced-motion support.

## 6. Verification evidence

### Compiler and test gates

- `npm run build`: **PASS** — TypeScript and Vite production build, zero errors.
- `cargo check`: **PASS** — zero errors and no remaining compiler warnings at the final source checkpoint.
- `cargo test`: **25/25 PASS**.
- Mechanical contract comparison: **11 shared Rust structs / TypeScript interfaces PASS**, with identical serialized field names.

The Rust tests cover provider payloads, OpenAI/Anthropic deltas, split UTF-8/CRLF SSE, a real local HTTP/SSE round-trip, exact edits/diffs, path containment, empty files, filtered context files, private-address rejection, legacy session migration, skill frontmatter, API-key redaction, real PowerShell streaming/cancellation, and a real MCP JSON-RPC child-process round-trip.

### Native UI smoke tests

Window-specific release captures and managed Windows UI Automation verified:

- Production custom-protocol UI loads without the localhost development-server error.
- Main shell, session sidebar, workspace control, model header, empty state, and composer render correctly.
- Model picker opens and exposes search, model rows, stars, provider filter, refresh, Thinking, custom-model, and settings-default controls.
- All seven effort options, including Ultra, are accessible and visible after the popover correction.
- Settings Providers and Skills & MCP tabs render without clipping.
- Slash commands open and show all four commands.
- `@` autocomplete returns relevant project files and excludes `.git` internals.
- Favorite changes persist to disk; the test state was restored afterwards.
- Per-session model selection changes the effective model and `Use Settings default` restores it.

## 7. Release artifacts

- Main executable: `D:\AetherAgent\AetherAgent.exe`
- Tauri executable: `D:\AetherAgent\src-tauri\target\release\aether-agent.exe`
- NSIS installer: `D:\AetherAgent\src-tauri\target\release\bundle\nsis\AetherAgent_0.2.0_x64-setup.exe`
- MSI installer: `D:\AetherAgent\src-tauri\target\release\bundle\msi\AetherAgent_0.2.0_x64_en-US.msi`

**Final executable SHA-256:** `83A063AB8F72B3745038552A641369EAB0E8C0C5BE9A8A7FA65979C157A9F412`  
**Final executable size:** `10,097,664 bytes`  
**Product/File version:** `0.2.0`

## 8. Performance and external-service boundaries

The implementation bounds mounted chat/artifact rows and avoids continuous graphics loops. It does **not** claim a universal 60 FPS or total-process memory below 60 MB without a workload-specific measurement that includes WebView2 child processes. Those are runtime benchmarks, not properties that a compiler can prove.

Provider integration was tested with local mock HTTP/MCP services. No external GutsAI, Anthropic, DeepSeek, or Ollama credentials were used or disclosed during verification. Actual remote-model availability, provider-specific support for every custom effort label, and remote quotas remain dependent on the configured service.
