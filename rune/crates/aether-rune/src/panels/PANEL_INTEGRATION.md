# Native panel integration contract (parent shell/state agents)

`panels.rs` owns `UiState` and `Panel`. Import `crate::panels::{UiState, Panel}` in the shell; do not declare another UiState. AppState must expose `controller: aether_state::Controller`, `ui: UiState`, and `rich: RefCell<RichContent>`.

Expected Controller methods: `model(&self) -> &Model`, `dispatch(&mut self, command: &str, args: serde_json::Value) -> u64`, `open_session(&mut self, session_id: &str)`, `open_message(&mut self, session_id: &str, message_id: &str)`. Open-message must project the bounded around-page into current state read-only; dispatching load_session_around alone is not equivalent to selecting a session. The panels use the actual current model schema (`results: BTreeMap<u64, CommandResult>`, `pending_commands: BTreeMap<u64,String>`), not an invented command_results field.

Call `state.ui.sync(&mut state.controller)` immediately after the controller drains completions and before requesting the next frame. This hydrates successful read results into editors and schedules refreshes *after* successful mutations. Never dispatch inside render. Toolbar buttons call `state.ui.open(Panel::Settings, &mut state.controller)` etc. `panels::render(&state, cx, kit)` returns None with no open panel.

UiState public fields include panel/forms/search/selected/result_cursor/expanded/image_preview/minimized. Persistent native TextEdit values are internally cached because Rune fields bind TextEdit rather than String. Forms are updated on native input changes.

Dependencies already implied by parent/rich: rune, rune-kit, aether-core, aether-state, serde_json. No manifest changed by panel agent. Password API is `.secure()` (not `.secure(true)`). Advanced settings redact keys before displaying and restore untouched secrets from the complete source document. Dedicated provider API-key input is secure.

Native OS save dialog can be added by parent host; this panel explicitly labels the export destination textbox fallback and requires an Export click. No pretend one-click dialog. Signed update installation is not exposed because core service explicitly requires a native updater host. Live Shell is one bounded, noninteractive command, not a PTY.

Tests live under panels/tests.rs and panels/domain.rs. Include module tests with the main crate test build. The domain tests can also run with rustc against existing serde_json dependency when the parent Cargo manifest/controller is not yet available.

## Verified live adjustments
The actual shell exposes AppState at crate::app::AppState. Controller does not yet expose open_message: history opens the session then dispatches load_session_around, which the current controller projects to Model.preview (read-only), rather than pretending it changes current. Actual results/pending_commands contracts are now present and used. All panel commands compile under the current contract. A cargo check on 2026-11-09 reached the shell and reported only two unsupported El.selected(...) calls in view.rs (outside panel ownership).

## Final verification
The parent fixed the shell selected-method blocker concurrently. Actual cargo test --manifest-path rune/crates/aether-rune/Cargo.toml --lib --offline -- --test-threads=1 was executed against the live shell/controller. The original 24 library tests passed, including 9 panel tests. Final safety regression adds strict secret restoration by provider identity and a real bounded-shell stdout round trip. Final cargo exit code: 0. Native library tests use isolated temporary profiles/workspaces, no production data or provider network calls. Advanced command forms cover the entire actual Backend::supported_commands inventory, but network MCP/provider operations and destructive Git/review decisions are not all end-to-end tested. Headless Runtime controls are exercised; no OS-window visual audit is claimed.
