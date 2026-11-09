# Native controller integration contract

`aether-state` is renderer-independent. The Rune parent compiles against this API.

## Construction and public model

- `Controller::new() -> Result<Self, String>` owns a four-worker Tokio runtime,
  a bounded lossless event queue, and `Arc<aether_core::service::Backend>`.
- `new_with_workspace(String)` canonicalizes an existing workspace and binds it
  without saving workspace configuration or changing process environment.
- `with_runtime(Arc<Runtime>)` shares a runtime; use at least four workers.
- `with_backend(runtime, |host| Arc<Backend>)` injects a backend factory using
  the controller's host. `from_backend(runtime, Arc<Backend>)` shares the injected
  runner/workspace while replacing its event host with the controller queue.
- The parent must select its native profile before construction. Constructors
  never select/reset the process-global profile or load/save settings implicitly.
- Public fields: `backend`, `runtime`, `model`. Accessors: `model()`, `model_mut()`.
- `Model.settings` is native `AgentSettings`; defaults are available. The parent
  initializes saved state with `dispatch("get_settings", {})` and `list_sessions`.

## Polling and operations

- `set_waker(Arc<dyn Fn() + Send + Sync>)` schedules repaint, never inline poll.
- `poll() -> bool` handles at most 256 queued events, schedules persistence, and
  publishes current history once after the batch, guarded by session/version.
  It works without any waker. Poll periodically while active (e.g. 250 ms) for
  the two-second incremental checkpoint cadence.
- `dispatch(&str, Value) -> u64` calls the native command facade, never a script
  fallback. Unknown commands retain native errors. Completion is transported as
  `controller:result` with `opid`, `command`, `sessionId`, `generation`, and the
  serde-encoded `Result` (`Ok`/`Err`). `model.results` holds native-shaped values;
  `last_command` and `take_result(id)` are also available. Results retain the
  origin session/generation; the retained result map is capped at 256.
- `send(String)`, `send_council(String, ProviderConfig, ProviderConfig)`, `abort()`,
  `older()`, and `choose_council(message_id, "A" | "B")` return `Result`.
  `new_session()` creates/selects an empty local session. `open_session(id)`
  returns an operation ID, or zero for an immediate cached selection.

## Chat persistence and navigation

Send snapshots effective settings and the per-session model override, prepares
explicit image inputs, and saves user + assistant placeholder before starting
native execution. Requests are deserialized from the complete settings snapshot;
provider context excludes the same-run placeholder and uses canonical hydration
with the acknowledged history cursor. Native backend owns runner completion.

Live streams, committed baselines, and partial-window cursors are separate.
Checkpoints are run-scoped incremental deltas with one in-flight persistence
operation per live session, monotonic sequence, and exact-version acknowledgement.
Acknowledgements do not republish history or mutate the committed baseline.
Terminal events enter Saving; final save is serialized after any checkpoint.
Idle/Completed/Cancelled/Error follows its acknowledgement, not merely runner done.
Failed saves preserve dirty data and report `save_failed` instead of silently rebasing.

Open uses a bounded 50-message tail, navigation-generation guards, and a cache
of eight clean inactive sessions. Active/dirty/in-flight sessions are protected
and may exceed that nominal cap. Older loads prepend contiguous prefix messages
and union artifacts without replacing streamed data. A cached selection cancels
pending navigation/loading. Full native loads retain their native result shape
while obtaining a real cursor, never inventing a revision.

`load_session_around` publishes `model.preview: Option<SessionPage>` separately
from the writable suffix. UI should render preview when present. `close_preview()`
returns to current history. Send, older, and council selection reject preview mode.
Metadata operations update cached/catalog summaries and reconcile revisions
against the acknowledged baseline; review events attach to matching cached turns
and schedule guarded native reconciliation even for turns outside the suffix.
External page saves reserve the live session until acknowledgement. Full saves
and full checkpoints are refused for paged sessions. Destructive operations are
refused for active/dirty sessions. Artifact reads do not overwrite streamed artifacts.

Dropping disconnects the event receiver before runtime teardown so a full queue
cannot deadlock shutdown, and requests cancellation of cached active runs. It is
not an asynchronous flush: hosts should poll through final save before shutdown.

## Verification and remaining boundaries

State tests include real local HTTP streaming + durable-before-provider placeholder,
final persistence, no-waker polling, 256-event batching, paged navigation/prefix
preservation, search/open races, metadata reconciliation, checkpoint sequencing,
immutable baselines, native result shapes/settings, cancellation before start,
external-save serialization, review outside suffix, and backpressure shutdown.

Core library and service-dispatch suites were also run. Parent compilation is
verified; this is not a claim of interactive native GUI QA or full Council/provider
matrix coverage. Generic raw send commands are forwarded native operations;
use controller send conveniences for live model/persistence bookkeeping.

Known core boundary: even with a disposable storage profile, native runner skill
discovery can inspect/initialize global skill roots. The HTTP test currently emits
two existing malformed global-skill YAML warnings. Correcting core skill-profile
routing is outside this state-only change. No automatic stale-dirty rebasing or
failed-save retry UI is implemented; conflict recovery remains explicit.
