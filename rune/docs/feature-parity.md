# Feature parity inventory: React/Tauri baseline to Rune

## Delivery status and evidence rules

**Incomplete native port. This is not full feature parity, release sign-off, or a
claim that the request for “all features, none missing” has been fulfilled.**
The previous all-`not-started` UI inventory was stale: an executable, native chat,
modal panels, history controller, attachments and export now exist. Conversely,
sharing a backend command, preserving a setting in JSON, or rendering a card does
not implement every baseline interaction.

This inventory covers the current working-tree baseline, including uncommitted
work. It accounts for **all 31 non-test files directly in `src/components/`**, the
app/hooks, composer commands and **63 registered baseline IPC commands**. It is a
source audit, not a fresh exhaustive interaction test. Final tests and final
executable/hash-specific QA remain pending with the parent integration task.

| Status | Meaning |
|---|---|
| `not-started` | No native equivalent of the stated behavior was found. Related widgets or backend primitives do not close the gap. |
| `backend-shared` | Authoritative backend implementation is reused; this is not native presentation or OS acceptance evidence. |
| `native-implemented` | A real native path exists at the cited source. It may be partial; read its explicit limits. Final acceptance is pending. |
| `verified` | Only the named narrow behavior has observed test/log evidence. Historical evidence is not certification of later edits. |

For compact source references below, `R/` means
`rune/crates/aether-rune/src/`, `S/` means
`rune/crates/aether-state/src/`, and `C/` means
`rune/crates/aether-core/src/`. Tests are in each crate's `tests/` or the named
inline test module. These are repository paths, not claims about installed builds.

## Cross-cutting implementation matrix

| Capability | Status | Source and acceptance boundary |
|---|---|---|
| Real native executable and startup | `native-implemented` | `rune/Cargo.toml`, `R/main.rs`, `R/cli.rs`, `R/app.rs::spec`: Rune/winit/wgpu, without a Tauri/WebView UI. Profile resolution/import/configuration precede controller creation. Windows debug evidence only; not a packaged cross-platform release. |
| Separate default profile and one-shot routing | `verified` | `src-tauri/src/agent/profile.rs`; six tests passed in `rune/profile-verified.log`, including pure resolution, child-process configure, Windows aliases/junctions and copy-only import. Host wiring is now present in `R/main.rs`; final executable startup regression remains a gate. |
| Send, streamed text/reasoning/tool/council state, stop | `native-implemented` | `S/controller.rs`, `S/reducer.rs`, `S/events.rs`, `R/view.rs::chat/composer`. Per-session run ownership and cancellation exist. Native error/run presentation is narrower than the baseline; a real external-provider acceptance run is not established by this audit. |
| Canonical history, bounded pages, preview and committed state | `native-implemented` | `S/controller.rs::open_session/older/close_preview`, `S/history.rs`; `R/view.rs` older-history/return-to-latest/timeline controls; `R/panels.rs::history_panel`. Revision/CAS and stale-navigation regression tests exist. Disk pagination is not proof of virtualized layout or stable anchors for every long-session case. |
| Edit/resend, retry/regenerate, undo and rollback | `native-implemented` | `S/history.rs`, `S/message_edit.rs`; `R/panels.rs::conversation_action/conversation_panel/message_menu`. Role-specific actions, confirmation, editor/draft preservation and image replay have test coverage. Native context menu exposes retry on user turns, not the baseline's full assistant-message action menu. |
| Offscreen runs, save failure and close handling | `native-implemented` | `S/controller.rs::schedule/retry_save/begin_shutdown/retry_shutdown_saves/cancel_shutdown`, `R/lifecycle.rs`, `R/main.rs`. Close waits for acknowledgments; timeout offers retry/keep-open/explicit loss-risk close. This was edited after earlier GUI QA: final close, failure, multi-window and process-exit tests are pending. |
| Models, defaults and stored per-session overrides | `native-implemented` | `R/panels.rs::models_panel/settings_panel`, `S/controller.rs::begin_send`. Configured provider/model discovery/editing exists; controller honors persisted model/thinking/effort overrides. Dedicated native per-session override/favorite/search UX is not equivalent to baseline `ModelPicker.tsx`. |
| Workspace, files, tools, skills, MCP, memory, Git | `native-implemented` | `R/native.rs::choose_workspace/attach_paths`, `R/panels.rs` domain panels and `C/service.rs`. Actual service requests, acknowledgment/error handling and scoped confirmations exist. Advanced command dispatch is not a substitute for missing purpose-built UX. Workspace/tool permissions are not sandboxed by profile isolation. |
| Real file/image attach, paste/drop and preview | `native-implemented` | `R/native.rs`, `R/view.rs`, controller image preparation/replay. OS file/folder dialogs, clipboard files/images, generation guards and decoded image preview exist. Final Windows clipboard-format, IME, drag/drop, Unicode-path and permission/error acceptance must still run. |
| Export complete native workflow | `native-implemented` | `R/export.rs`, `S/commands.rs`, `C/service.rs::export_session`: JSON, Markdown and HTML; save dialog or explicit-path confirmation, cancellation, scope guards and real backend writes. `tests/export.rs` exercises these paths. This is implemented, not merely a backend-shared placeholder; final OS dialog acceptance is still pending. |
| Markdown, highlighted code, links and native math | `native-implemented` | `R/rich_content.rs`: RuneKit Markdown, protected-source normalization, bounded async `latex-rust` SVG rendering, exact-source copy and visible unsupported fallback. Native inline equation presentation is not baseline inline KaTeX layout; supported academic examples are not full LaTeX/KaTeX parity. |
| Keyboard/focus/accessibility/responsive presentation | `native-implemented` | `R/app.rs::on_key`, `R/view.rs`, `R/panels.rs`, `R/widgets.rs`; native roles/labels, modal/menu focus behavior and reduced-motion path exist. No full screen-reader/high-contrast/IME/keyboard-only acceptance. Several baseline shortcuts and Spotlight remain missing. |
| Notification, sound and dictation | `not-started` | No native notification delivery, chime playback/test or speech-recognition controller is wired. Persisted notification/sound fields do not implement these integrations. Baseline `audioChime.ts` is muted by default; `useSpeechRecognition.ts` is optional Web Speech, not an always-available feature. |
| Global Alt+Space | `not-started` | Baseline `src-tauri/src/lib.rs` registers a global shortcut. `R/app.rs::on_key` is local window key handling; `R/main.rs` has no equivalent global registration/toggle integration. |
| Signed updater, installer and platform distribution | `not-started` | `C/service.rs` explicitly rejects update check/install without a native updater host; About disables installation. Shared release policy/info does not install anything. No Rune Linux/macOS build/package acceptance or signed Windows installer is established. |

## Complete baseline component inventory (31 files)

A `native-implemented` row denotes the **implemented subset**, not a complete
screen certification. The last column states what must be compared or built next.
Baseline paths in this table are relative to `src/components/`.

| Baseline file | Status | Native source / implemented subset | Missing behavior or next acceptance test |
|---|---|---|---|
| `AboutModal.tsx` | `native-implemented` | `R/panels.rs::about_panel`, `C/service.rs::release_info`: real release information and log-path actions. | Native signed update host absent; installation disabled. Test info/errors/log opening; do not advertise updater parity. |
| `AetherLogo.tsx` | `not-started` | Native sidebar/header branding text exists in `R/view.rs`, not the baseline logo component. | Port/compare SVG branding, sizes and accessible treatment. |
| `ArtifactPane.tsx` | `native-implemented` | `R/panels.rs::artifacts_panel`, rich content and real artifact reads/open/reveal actions. | Native aligned side-by-side diff not built; compare every artifact kind, source copy/download and large terminal payload. |
| `ArtifactsLibraryModal.tsx` | `native-implemented` | `R/panels.rs::artifacts_panel`: cross-session catalog, selection and lazy content read. | Compare baseline filtering/search, empty/error/loading states and stale reads across navigation. |
| `AttachmentPreview.tsx` | `native-implemented` | `R/native.rs`, `R/view.rs`: chips/remove, image decoding/cache, preview modal, preparation/cancel/error paths. | Genuine OS paste/drop and replay of missing/changed images; screen-reader and focus acceptance. |
| `BackgroundTasksCard.tsx` | `native-implemented` | `R/panels.rs::tasks_panel`: session-scoped list, output tail, refresh and **actual task kill**, with confirmation. | Exercise real process termination, wrong-session refusal and teardown; do not describe this as status-only. |
| `ChatCanvas.tsx` | `native-implemented` | `R/view.rs`, `R/app.rs`, `R/native.rs`, `S/commands.rs`: composer/send/stop, attachments, slash dispatch and bounded history. | Queued prompts and `@` file autocomplete absent; full slash suggestion/dynamic-skill UX absent. Compare long-session layout/scroll/search/IME. |
| `ContextMenu.tsx` | `native-implemented` | `R/panels.rs::message_menu`: raw **message source**/plain-text copy, user edit/retry, assistant rollback, latest-turn undo. | Not raw-turn JSON copy. Selection quote/explain/refactor/audit/summarize/translate, run-selection prompt, cut/paste-as-code/select-all menu, code tests/audit/save prompts and context-sensitive link/file/global actions are not ported. Native keyboard editing or other panels are not this full menu. |
| `CouncilView.tsx` | `native-implemented` | `R/view.rs::chat`, `S/controller.rs::send_council/choose_council`: candidate presentation/selection and stored provenance. | Test two actual streams, candidate failures/reasoning/tools, winner persistence and later replay. |
| `ErrorBoundary.tsx` | `not-started` | Composer/panels show operation errors; math worker catches rendering failures. Neither is a general UI error boundary. | Define native render/panic containment and recovery without pretending a banner equals React boundary behavior. |
| `ExecutionTrace.tsx` | `native-implemented` | `R/view.rs::chat`: tool status, args/results/terminal output and artifact links. | Cards are all expanded with unbounded visible details; no phase-scoped collapse/auto-collapse/progressive disclosure equivalent. Stress-test large traces. |
| `FilesChangedCard.tsx` | `native-implemented` | `R/panels.rs::git_panel`: actual status, stage/revert, commit/fetch/push requests and confirmations. | Disposable-repository tests for failures, workspace scope and refresh; compare baseline inline file/diff presentation. |
| `InteractivePlanCard.tsx` | `not-started` | `/plan` prompt expansion exists in `S/commands.rs`; generic Markdown task editing exists in `R/rich_content.rs`. | A fenced `plan` block remains a regular code fence, not the baseline three-state progress/checklist card. Baseline source represents pending/in-progress/done and reconciles local toggles; it is not evidence of a separate backend approval/execution protocol. Test repeated steps and streamed source updates. |
| `InteractiveTerminal.tsx` | `native-implemented` | `R/panels.rs::shell_panel`: real bounded noninteractive workspace command and output; OS terminal launch. | Not a PTY or persistent interactive shell. Compare the baseline's actual input/history/output UX rather than claiming full terminal emulation. |
| `markdownMath.ts` | `native-implemented` | `R/rich_content.rs::Document`: four delimiters, protected code/links/images/HTML, escaped money and incomplete-input fallback; original source retained. | Broader baseline normalization fixture comparison, malformed/nested/mixed-language edge cases and streamed fragments. |
| `MarkdownRenderer.tsx` | `native-implemented` | `R/rich_content.rs`, RuneKit Markdown: GFM/code/diff-source/task widgets, native SVG math/cache/fallback and copy. | Inline math cards/layout differ from KaTeX baseline. Supported `latex-rust` subset, not packages/arbitrary macros/full academic parity. Compare matrices/cases/aligned equations, nesting, overflow and screenshots. |
| `MemoryModal.tsx` | `native-implemented` | `R/panels.rs::memory_panel`: actual AGENTS.md read/init/edit/save/preview, expected-workspace guard and acknowledgment refresh. | Test external edits, denied writes, stale workspace and modal keyboard flow. |
| `MessageRow.tsx` | `native-implemented` | `R/view.rs::chat`, message menu and rich renderer: roles, content/reasoning/tools/images/council and history actions. | Segment ordering/provenance/timestamp presentation, exact-source selection and streaming memoization/performance require comparison; action menu is only a subset. |
| `ModelPicker.tsx` | `native-implemented` | `R/panels.rs::models_panel`: configured provider selection/add/edit, model identifiers and fetched model lists. | Dedicated favorites/search and per-session thinking/effort/override UX incomplete; full provider protocols/headers/custom lists use advanced JSON. |
| `OptionalPanel.tsx` | `native-implemented` | `R/view.rs` overlays, `R/panels.rs::render`, RuneKit modal/menu mechanisms and pending/error presentation. | Final nested-modal focus containment/restoration, Escape, resize and keyboard-only acceptance. |
| `RunStatusBar.tsx` | `native-implemented` | `R/view.rs::composer`, `S/model.rs`/controller: phase and save-failure presentation, stop/cancellation state. | Baseline timing/provenance/answer/files/verification outcome detail not equivalent. Retry-save controller exists; composer banner is not a general Retry All action. |
| `SelfImprovementBadge.tsx` | `native-implemented` | `R/panels.rs::review_panel`, controller review events: actual proposals/diff reads and scoped approve/reject. | Compare linked skills/change history, background updates, failure handling and diff fidelity; native diff source is not aligned side-by-side UI. |
| `SessionHistoryModal.tsx` | `native-implemented` | `R/panels.rs::history_panel`: list/deep search/open, rename/pin/compact/delete/export, turn actions. | Compare grouping, stable selection, keyboard behavior, navigation races and all export entry points. |
| `SettingsModal.tsx` | `native-implemented` | `R/panels.rs::settings_panel/models_panel`, `panels/domain.rs`: basic controls plus typed full JSON merge, secret redaction/restoration and acknowledged save. | JSON preserves configuration but does not reproduce full semantic tabs/validation UX for protocol, headers, reasoning, review, execution, skills and MCP/env. Notification/sound settings have no native effects. Pending initial get-settings may replace edits; test fixture synchronization fix is not a production loading barrier. |
| `Sidebar.tsx` | `native-implemented` | `R/view.rs::sidebar/header`: new/select/search sessions, workspace dialog, history/settings access and narrow-layout toggle. | Session mutations/Git/export live in panels, not equivalent inline sidebar controls. Compare overlay dismissal and keyboard/resize behavior. |
| `SideBySideDiff.tsx` | `not-started` | RuneKit/code-frame diff-source rendering via `R/rich_content.rs` and panels is available. | No native aligned original/modified side-by-side hunk viewer; test labels, line alignment, scrolling and exact source after implementation. |
| `SpotlightBar.tsx` | `not-started` | Buttons and slash intents open native panels; no native searchable command/navigation palette. | Implement real command dispatch, keyboard selection and Escape/focus restoration; Ctrl+K is not automatically covered by Models. |
| `SubagentTreeCard.tsx` | `native-implemented` | `R/view.rs::composer`, `S/reducer.rs`: role/status/task/summary cards and subagent state. | Flat role cards, not parent/child tree inspection or full active-skill/result detail. Compare nested agents and failures. |
| `TerminalOutput.tsx` | `native-implemented` | `R/view.rs::chat`, artifact/shell panels: real terminal text/result presentation. | Full ANSI formatting, bounded output view, autoscroll and huge-output performance are not certified; open tool output is unbounded visually. |
| `TimelineScrubber.tsx` | `native-implemented` | `R/view.rs::timeline`: turn selection/jump alongside bounded history/preview controller. | Not equivalent continuous scrubber/scroll-position synchronization; test hidden turns, prepend anchors, resize and reduced motion. |
| `ToolsInspectorModal.tsx` | `native-implemented` | `R/panels.rs::tools_panel`: native schemas, enabled MCP discovery/call validation, skills list/save and scoped results. | Exercise real MCP process errors/timeouts, secret handling, focus and server switching; settings JSON is not complete semantic MCP editor parity. |

## App, hooks and non-component behavior

| Baseline source / boundary | Status | Native evidence / remaining acceptance |
|---|---|---|
| `src/App.tsx`, `main.tsx` | `native-implemented` | `R/main.rs/app.rs/view.rs`: native host/layout/overlays and local shortcuts. `on_key` handles new/sidebar/settings/history/reduced-motion/send/Escape paths; baseline Ctrl+K command palette and global Alt+Space are not implemented. Full key/focus mapping remains a gate. |
| `src/hooks/useAgent.ts` | `native-implemented` | `S/controller.rs`, `commands.rs`, `history.rs`, `message_edit.rs`, `C/service.rs`; actual startup, run/history/persistence/actions, not a reducer prototype. Native notifications and several presentation/actions remain absent. |
| `src/hooks/useAgentLiveSession.ts` | `native-implemented` | `S/model.rs/controller.rs/reducer.rs/events.rs`: offscreen owners, committed/live versions, checkpoints, bounded event draining, cancellation and save recovery. Test later edits and final close behavior; no blanket event/lifecycle certification. |
| `src/hooks/useHistoryPreview.ts` | `native-implemented` | Controller around-message preview, stale-navigation guards, older reads and return-to-latest. Test concurrent offscreen streaming, paged mutation, and visible anchoring. |
| `src/hooks/useIsNarrowViewport.ts` | `native-implemented` | `R/view.rs` layout-size/compact branches and sidebar toggle. Test minimum size, overlays and multi-monitor scaling. |
| `src/hooks/useModalAccessibility.ts` | `native-implemented` | Rune/RuneKit dialogs and menus plus labels/roles in native view/panels; runtime focus/Escape tests exist. Full OS accessibility tree and assistive-technology acceptance pending. |
| `src/hooks/useSpeechRecognition.ts` | `not-started` | No native dictation start/stop/permission/transcript implementation; baseline Web Speech is optional and platform-dependent. |
| `src/utils/sessionCacheBudget.ts` | `native-implemented` | `S/controller.rs::evict` bounds inactive clean sessions and measured bytes while retaining protected live/dirty state. Verify budget/eviction and long-session memory; not a full UI layout cache audit. |
| `src/utils/audioChime.ts` | `not-started` | No native chime or OS notification wiring. Baseline optional chime defaults OFF; preserving its flag does not produce playback. |
| `src/index.css` / motion | `native-implemented` | Native Kit theme/layout and `reduced_motion` path. Baseline visual fidelity, contrast, scaling, math/code overflow and accessible reduced-motion UX are not signed off. |

### Composer command dispatch and help surface

`S/commands.rs` is a real command router consumed by `R/app.rs::consume_ui_actions`,
not merely a catalog of synthetic provider prompts:

| Commands | Status | Native effect / boundary |
|---|---|---|
| `/new`, `/clear`, `/undo`, `/retry`, `/title`, `/stop` | `native-implemented` | Local controller/history actions with idle/preview/mutation guards. Literal stored prompts are not replayed as local commands. |
| `/compact`, `/compress` | `native-implemented` | Real compaction dispatch. |
| `/review`, `/reflect`, `/refine` | `native-implemented` | Real review dispatch after successful-turn validation, not an ordinary chat message. |
| `/history`, `/settings`, `/model`, `/tasks`, `/tools`, `/skills`, `/memory`, `/init` | `native-implemented` | Native panel intents; `/init` exposes an explicit memory initialization action, not a silent write. Exact baseline argument/autocomplete behavior still needs comparison. |
| `/export`, `/save` | `native-implemented` | JSON/Markdown/HTML intent consumed by `R/export.rs`; save chooser, explicit-path confirmation and backend result. |
| `/plan` | `native-implemented` | Actual run prompt expansion. The plan **card** remains absent; do not promote `InteractivePlanCard` on this basis. |
| `/agent`, `/ask` | `native-implemented` | Request-only execution-mode/prompt overrides without changing saved defaults. |
| `/pin`, `/artifacts`, `/terminal`, `/git` | `native-implemented` | Additional native command branches; not evidence of missing baseline UX being covered elsewhere. |
| `@` workspace-file autocomplete, dynamic skill suggestions, full slash-menu help | `not-started` | Attach/file chooser and shared `list_workspace_files` are not caret-aware mention resolution or suggestion navigation. |
| Queued instructions while streaming/reviewing | `not-started` | Baseline `ChatCanvas.tsx` has origin-scoped queue/edit/remove/resume; native composer has no equivalent user-prompt queue. The bounded **UI action queue** in `S/commands.rs` is unrelated. |

Baseline empty-state/help hints, `ContextMenu.tsx`, plan parsing and
`SettingsModal.tsx` are acceptance sources, not proof of features just because
wording appears in a tooltip. In particular, the baseline Alt+Space hint corresponds
to real Tauri registration, while optional voice/audio depend on web capabilities.
No native help/palette completeness claim is made.

## Complete registered IPC inventory (63 commands)

Baseline registration is `src-tauri/src/lib.rs::generate_handler!`. Native service
implementation is `C/service.rs::Backend::dispatch/supported_commands`; shared
modules are path-reused through `C/agent.rs`. Status here describes the **service
boundary**, not the UI feature above. Updater entries deliberately retain an
explicit unsupported status. Generic advanced dispatch cannot certify a feature.

| Baseline command | Status | Native implementation / boundary |
|---|---|---|
| `get_release_info` | `backend-shared` | `service.rs::release_info`; About displays data; not a signed installed-build verification. |
| `check_release_update` | `not-started` | Service returns unsupported without a native updater host. |
| `install_release_update` | `not-started` | Service returns unsupported; no Rune install/restart transport. |
| `send_message` | `backend-shared` | Shared runner/provider/events; native send controller exists. |
| `review_session` | `backend-shared` | Shared review runner; native command/review panel exists. |
| `read_reflection_skill` | `backend-shared` | Shared review/skill reads; verify linked-skill UX. |
| `list_review_proposals` | `backend-shared` | Native Review panel consumes proposals. |
| `review_proposal_diff` | `backend-shared` | Native diff-source preview; not side-by-side viewer. |
| `approve_review_proposal` | `backend-shared` | Native scoped confirmation and acknowledgment. |
| `reject_review_proposal` | `backend-shared` | Native scoped confirmation and acknowledgment. |
| `send_council_message` | `backend-shared` | Shared runner, native candidate UI/controller. |
| `abort_agent` | `backend-shared` | Controller/Stop/shutdown cancellation. |
| `get_settings` | `backend-shared` | Native startup/panels load asynchronously; edit-before-load risk remains to test. |
| `save_settings` | `backend-shared` | Native typed JSON/basic/provider save and result handling. |
| `list_sessions` | `backend-shared` | Native sidebar/history catalog. |
| `warm_session_index` | `backend-shared` | Index primitive; warmup scheduling/budget acceptance remains distinct. |
| `load_session` | `backend-shared` | Full canonical load under controller ownership. |
| `load_session_page` | `backend-shared` | Native bounded open/older paths. |
| `load_session_around` | `backend-shared` | Native read-only search/turn preview. |
| `save_session_page` | `backend-shared` | Controller-owned revision-aware persistence, not arbitrary UI write. |
| `refresh_session_window` | `backend-shared` | Controller revision/refresh coordination. |
| `checkpoint_session_page` | `backend-shared` | Controller-owned checkpoint/acknowledgment. |
| `read_session_artifact` | `backend-shared` | Native lazy artifact reads. |
| `read_message_image_inputs` | `backend-shared` | Native replay/hydration path. |
| `rename_session_summary` | `backend-shared` | Native history/title actions. |
| `save_session` | `backend-shared` | Controller-owned full persistence; paged suffix cannot replace full canonical data. |
| `checkpoint_session` | `backend-shared` | Shared journal primitive, controller-owned lifecycle. |
| `checkpoint_session_delta` | `backend-shared` | Versioned delta/checkpoint controller path. |
| `rename_session` | `backend-shared` | Shared metadata action; prefer native guarded summary route. |
| `toggle_pin_session` | `backend-shared` | Native History and `/pin`. |
| `list_all_artifacts` | `backend-shared` | Native artifact catalog. |
| `deep_search` | `backend-shared` | Native History search/preview. |
| `delete_session` | `backend-shared` | Native scoped destructive confirmation. |
| `export_session` | `backend-shared` | Full native export workflow exists; JSON/Markdown/HTML backend writer. |
| `fetch_provider_models` | `backend-shared` | Native Models panel discovery. |
| `get_workspace_cwd` | `backend-shared` | Startup and native Shell/workspace views. |
| `set_workspace_cwd` | `backend-shared` | Native folder dialog and explicit path control. |
| `list_workspace_files` | `backend-shared` | Primitive available; native `@` autocomplete absent. |
| `list_skills` | `backend-shared` | Native Tools/skills panel; dynamic composer suggestions absent. |
| `save_skill` | `backend-shared` | Native explicit skill save/confirmation. |
| `list_native_tools` | `backend-shared` | Native Tools schema inspection. |
| `list_mcp_tools` | `backend-shared` | Native configured-server discovery; real process acceptance pending. |
| `call_mcp_tool` | `backend-shared` | Native enabled-server/JSON argument checks; real server acceptance pending. |
| `git_status` | `backend-shared` | Native Git panel. |
| `git_commit` | `backend-shared` | Native Git panel; real disposable-repository acceptance needed. |
| `git_push` | `backend-shared` | Native action; remote/auth/error acceptance needed. |
| `git_fetch` | `backend-shared` | Native action; remote/auth/error acceptance needed. |
| `revert_file_changes` | `backend-shared` | Native path-scoped confirmation. |
| `stage_file_changes` | `backend-shared` | Native path-scoped action/refresh. |
| `run_shell_command` | `backend-shared` | Native bounded command form, not PTY. |
| `compact_session` | `backend-shared` | Native History and slash action. |
| `init_workspace_agents_md` | `backend-shared` | Native Memory explicit initialization. |
| `read_workspace_agents_md` | `backend-shared` | Native Memory read/preview with expected-workspace guard. |
| `save_workspace_agents_md` | `backend-shared` | Native Memory confirmation/write/refresh. |
| `inspect_paths` | `backend-shared` | Native attachment inspection. |
| `save_pasted_attachment` | `backend-shared` | Native clipboard image persistence, scope-bound. |
| `prepare_image_attachments` | `backend-shared` | Native preparation/replay/preview; final OS roundtrip pending. |
| `open_path_or_url` | `backend-shared` | Native link/artifact actions; OS/safe-scheme acceptance needed. |
| `reveal_in_explorer` | `backend-shared` | Native artifact/workspace/log actions; Windows acceptance needed. |
| `open_terminal` | `backend-shared` | Native workspace terminal launch, distinct from embedded command output. |
| `list_background_tasks` | `backend-shared` | Native task panel, session scoped. |
| `kill_background_task` | `backend-shared` | Actual native confirmed task termination request. |
| `get_background_task_output` | `backend-shared` | Native bounded tail request/result. |

Controller-only history operations are additional native routes; their presence
is not an omitted baseline IPC row. Conversely, raw save/checkpoint names exposed
by a service do not bypass controller lifecycle ownership and revision guards.
There is no identified `retry_all` service command in this 63-command registration;
verify the baseline's precise recovery behavior rather than claiming that the
native error banner or shutdown-save retry implements a general Retry All UI.

## Event, profile and persistence boundaries

- `C/agent.rs` reuses authoritative runner/provider/types/storage/event modules.
  `S/events.rs/reducer.rs/controller.rs` now consume run/session-scoped chunk,
  reasoning, tool, terminal/artifact, subagent/skill, council, error/done and review
  state. Event existence is not evidence that every native visual branch is equal.
  Test stale/duplicate terminal events, backpressure, offscreen completion and
  shutdown/restart recovery against the final build.
- `R/main.rs` parses CLI, resolves the root, optionally imports, configures storage
  **once**, then constructs the native app. Default is `~/.aether-agent-rune`;
  baseline `~/.aether-agent` is never selected as the write root. Import is explicit,
  copy-only consent, not permission to write production data.
- Import allowlist: `config.json`, `workspace.json`, canonical `sessions/*.json`
  and matching valid recovery checkpoints. Session IDs, schema, journals and
  canonical stamps are validated; nonempty/overlapping targets and reparse paths
  are refused. Derived SQLite projection, tasks/vectors, arbitrary skills,
  attachments and logs are **not** copied. This is not whole-user-data migration.
- Imported API keys, workspace/skill/attachment paths, MCP commands and execution
  settings remain sensitive. Profile isolation is not a tool/filesystem sandbox.
  Close the source application/use a private snapshot; startup-time path checking
  does not prove hostile namespace replacement or hard-link safety. Partial failed
  output is retained, not deleted or silently overwritten. See
  [safe profile contract](../README.md#safe-profile-contract).

## Evidence ledger: historical, not final-source sign-off

No Cargo suite or desktop session was launched by this documentation-only update.
The following existing artifacts were inspected; later implementation changes
must be tested again. Test source references document acceptance coverage, not an
implicit claim that this audit executed them.

| Evidence | Narrow observation and limits |
|---|---|
| `rune/profile-verified.log` | `verified`: six profile tests passed on Windows, including actual junction cases and fresh-process one-shot routing. Not Linux/macOS symlink or final GUI startup acceptance. |
| `rune/history-integration-final-state.log` | Historical successful test binaries: 28, 5, 7 and 11 passes. Corresponding controller/history/edit regression sources exist. Not certification of edits made after this run. |
| `rune/history-integration-final-core.log` | Historical shared-core result: 366 passed, 2 ignored. Not native UI or distinct independent coverage for every shared-source test binary. |
| `rune/shell-test-first.log`, `shell-smoke.log` | Historical native test binaries and successful fresh temporary-profile shell smoke. `--smoke` constructs native state/new session; it does **not** open a window or contact an LLM. |
| `verification/native-rich-content/gate-finisher-handoff.md` | Reports two successful locked/default-parallel workspace runs, each 884 passing executions / 4 ignored, and smoke success. Handoff explains duplicated backend-source tests: 521 passing and 2 ignored distinct fully qualified names. These are reported earlier-run totals, not a suite executed or final-source gate certified by this documentation audit. Consult its logs/summary before using counts in a final report. |
| Same test-gate handoff | Reports strict workspace Clippy failure and workspace formatting failure, with exact files/diagnostics. Neither gate is green on that evidence. Fixture startup-settings synchronization was fixed; production edit-before-load remains a separate review question. |
| Earlier `rune/core-final-tests.log` / `core-final-serial-tests.log` | Earlier parallel run failed SQLite registry-contention timing test (363 passed, 1 failed, 2 ignored); serial passed (364 passed, 2 ignored). Later handoff reports a test-only contention harness repair and green repeats. Do not hide the earlier failure or describe it as the final current result. |
| `verification/rune-qa-20261109-032751/` | Prior native QA artifacts exist; provisional `032314` trial has an audit-correction pointer. Neither directory establishes acceptance of later close/lifecycle/source changes or the final executable hash. Final parent QA must identify the actual build, profile, actions and exclusions. |
| `R/panels/tests.rs`, `R/rich_content_tests.rs`, native `tests/export.rs`, `tests/lifecycle.rs` | Runtime interaction tests cover settings validation/acknowledgments, menu copy/focus/edit, GFM/math/task widgets, export cancel/scope/write/failure and close policy. Runtime/injected clipboard/dialog tests are not equivalent to genuine OS clipboard/save-dialog or screen-reader acceptance. |
| State `tests/history_commands.rs`, `tests/slash_commands.rs`, inline controller regressions | Include CAS/navigation/image replay, local command versus prompt behavior, loopback provider requests, save recovery, cache budget and shutdown handling. Loopback fixtures are not live authenticated external-provider tests. |

### Specific remaining blockers and next tests

1. **Final integration gates:** rerun locked workspace tests with normal parallelism,
   profile/feature-off safety gates, strict Clippy and formatting after all parent
   edits. Record command, exit status, counts, source revision and executable hash;
   keep ignored/duplicate executions separate. Old QA is not final QA.
2. **Configuration startup race:** `UiState::open/load_settings/sync` may seed from
   defaults and then replace fields on an asynchronous `get_settings` completion;
   controller startup also loads settings asynchronously. Simulate immediate user
   edits/save before load, slow/error responses and reopen; decide on a loading or
   dirty-editor barrier. Fixing the shell test's fixture ordering is not that fix.
3. **Rich content:** compare baseline `MarkdownRenderer.tsx/markdownMath.ts` fixture
   corpus and screenshots against `R/rich_content.rs`: inline/display placement,
   code protection, currency, matrices/cases/aligned equations/macros, unsupported
   fallback, copy/export exactness, nested tables/lists and streaming overflow.
   `latex-rust` SVG support is real but a subset; no full KaTeX-equivalence claim.
4. **Unported interaction surfaces:** baseline `ContextMenu.tsx`, queued instructions
   and mentions in `ChatCanvas.tsx`, `InteractivePlanCard.tsx`, `ExecutionTrace.tsx`,
   `SubagentTreeCard.tsx`, `SpotlightBar.tsx`, `SideBySideDiff.tsx` define the exact
   remaining work. Build and test these behaviors rather than renaming generic
   code blocks, flat cards or command forms as parity.
5. **OS features:** wire native speech, optional muted-by-default chime/test,
   notifications and global Alt+Space; test permissions, focus/background behavior
   and cleanup. Validate real clipboard text/files/images, drag/drop, dialogs,
   safe links, reveal/terminal launch, IME and long/Unicode paths on Windows.
6. **Lifecycle/recovery:** test close during startup/preparation/streaming/save,
   offscreen dirty sessions, persistent disk failure/conflict, timeout/retry,
   keep-open and explicit loss-risk close; include two windows and actual process
   exit. `R/lifecycle.rs`/controller tests are not final OS-close evidence.
7. **Performance/accessibility/platforms:** long histories and large open traces,
   anchoring/eviction/memory/frame budgets; actual screen-reader tree, focus,
   keyboard-only, high contrast and scaling. Build/test Rune on Linux/macOS before
   claiming those ports; baseline Linux artifacts do not count as Rune builds.
8. **Distribution:** implement a Rune signed updater host and packaging/signing/
   install/restart/rollback path, then test installed artifacts. About/service
   information alone cannot close this release blocker.

**Conclusion:** substantial native implementation exists, and export is no longer
missing. Full baseline parity and final-build acceptance remain incomplete.
