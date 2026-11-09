# History integration completed in production crates

Controller hooks, async history execution and completion now run in the actual
state crate (the old copied harness is not used). HistoryAction supports Undo,
Clear, Rollback { message_id }, Retry { message_id }, Edit { message_id, text }.
UI dispatch names are intercepted and require the selected session ID. Internal
history and native replacement commands cannot bypass typed controller guards.

Native storage::replace_session_checked accepts a full hydrated baseline cursor
(or explicit None for create-only), takes the keyed session lock, validates its
current opaque revision, and writes through save_session_file. It returns FULL
stored data with replay bytes plus offset-zero window, not a thin viewport. No
canonical deserialize/readback occurs after the write. A post-commit projection
error is explicit, not a fabricated revision or successful acknowledgment. The
controller retains trimmed dirty state, the old immutable acknowledged baseline,
final_save failure phase, and an error result; automatic save/replay is suppressed.

Undo removes the latest user turn and restores replay fields without provider
calls. Rollback keeps the addressed message inclusive. Retry removes the selected
turn from its preceding user prompt. Edit requires a user ID. Artifacts linked to
retained messages survive; removed-turn artifacts and newer unlinked artifacts
are trimmed. Clear removes messages and artifacts explicitly.

History reserves persistence_busy and captures identity/version/run/navigation.
Native acknowledgment always updates the origin cache. Composer restoration and
automatic Retry/Edit resend require unchanged selection AND navigation generation.
The commands agent routes composer commands; automatic replay uses its existing
literal send_prompt entry point so saved slash prompts cannot execute local commands.
Model.replay_image_inputs stores canonical replay bytes. Send reuses only bytes
whose source_path matches actual non-directory attachments, prepares new paths,
then validates combined image budgets and signatures. No Rune UI files changed.

Validation commands (see logs under rune/):
- cargo test -p aether-state --no-fail-fast
- cargo test -p aether-core --lib -- --test-threads=2
- cargo check -p aether-state

New tests cover stale and competing native CAS writers, hidden-prefix undo,
foreign UI IDs, /undo replay fields, real local HTTP replay after deleting an
image file, navigation-suppressed HTTP for Retry/Edit, post-canonical dirty/error
handling, and matching-attachment-only saved/new image merging. Native projection
failure completion is fault-injected at the controller boundary, not at SQLite.