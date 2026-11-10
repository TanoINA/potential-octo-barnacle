# Session history pagination

## Design

History pagination limits what the UI loads and renders. It does **not** truncate the transcript on disk or the context sent to the provider.

The canonical format remains `SessionData` with the complete `messages` array. A `.history-index.json` sidecar stores top-level message offsets, byte lengths, and lightweight summaries such as a label, tool count, role, and run status. The index is a disposable cache, not a new storage format. If it is missing, stale, or corrupt, the native layer validates and rebuilds it from the canonical JSON.

On the normal open path, the native layer reads the index and only the requested message range. It does not deserialize the full transcript again. Rebuilding the index is a one-time full read when the cache is invalid. The sidecar also stores the authoritative revision and message count for the window, so a tail write can check for conflicts without rereading a multi-gigabyte transcript. Updating an unchanged committed baseline does not rebuild the index.

## Native API

- `load_session_tail`: returns session metadata, the newest window, and `SessionHistoryWindow`.
- `load_session_page`: reads an older range using a native cursor (`startIndex`).
- `save_session_page`: atomically splices an untouched canonical prefix with the current window and tail. Partial saves must include both a cursor and the canonical prefix revision.
- `load_session` and `load_session_for_run`: still return the full transcript for legacy consumers, export, clone, compact, undo/fork, and provider hydration.

Initial history windows and page chunks are limited to at most 100 turns/messages per native request. Cursors use the number of canonical messages, not the number of turns after `windowConversationMessages` has merged tool continuations.

### Partial-save rules

1. A UI window must never go through legacy `save_session` or `save_session_checked`. The Rust command rejects it rather than overwriting the canonical prefix with a partial snapshot.
2. A cursor alone is not enough: a partial save must carry a revision and match the canonical suffix anchor.
3. If a save is rejected, the UI window stays marked partial and dirty. Rejection must not be treated as a successful full save.
4. Fork, compact, undo, clone, and run startup must wait for the persistence queue. A failed pre-save prevents the operation from continuing.
5. While a run is active, canonical journal progress and provider replay remain authoritative.
6. A successful `load_session_for_run` hydration does not mean a later persistence step succeeded. The client merges the acknowledged baseline with the current active tail; it never replaces the current live transcript with the pre-run hydration snapshot.
7. If the primary session JSON cannot be replaced during a journal-backed save, a durable materialized head becomes the canonical recovery source. `load_session`, tail/page APIs, and the session list all read the same selected head.
8. Eviction removes only clean, idle live-session state. Dirty, saving, or running sessions stay in memory until they can be released safely.

## Frontend

- The frontend opens a session with `load_session_tail`, loads older pages with `load_session_page`, and persists a partial window with `save_session_page`.
- The default settings are `historyInitialTurns = 30` and `historyPageChunkTurns = 20`. Both are clamped to `1..=100` when settings are loaded or saved, not just in the UI.
- The load-more indicator reads `Loading older messages...` while a request is active. A button remains available as a keyboard-accessible fallback.
- The top sentinel only runs automatic pagination after a real user scroll event. The observer does not recursively fetch history just because a short session's initial viewport already intersects the sentinel.
- Scroll intent is reset by `sessionId` and `scrollResetKey`. Restoring a session through the viewport cache does not count as a user scroll gesture.
- The virtualizer captures an anchor before the prepend-triggering request and retains it until the prepend is applied. It then measures the anchor delta while the pre-prepend virtual slice is still rendered. Tail-follow is temporarily disabled, and only then are spacer estimates and the virtual slice updated.
- The anchor uses a stable canonical message ID and a viewport-relative offset. If its DOM node is recycled, the virtualizer reconstructs the anchor from the message index and height cache rather than using a raw scroll-height delta.
- Late `ResizeObserver` measurements, delayed text reflow, and image loading reapply the reading anchor when the reader is away from the tail.
- Full history can still be loaded with `load_session_for_run` when resuming a run. While a history window is partial, the UI says `Partial history; full context loads on run` instead of presenting its context estimate as complete.
- Export and clone load canonical history on the native side, not the UI's current window.
- `pageSessionHistory(sessionId)` reads the canonical cursor and updates that session's live record. A late page response never overwrites whichever session the user has since opened.
- A background page result can merge with a still-valid hydrated run prefix. If the history changed in a way that invalidates the cursor, the result is discarded without replacing the newer transcript.
- Unchanged prefix messages remain referentially stable: provider-state, tool, and status updates replace only the active tail.

## Conflict and recovery behavior

If a checkpoint conflict occurs and refreshing the latest canonical state also fails:

- The existing `historyWindow` is retained, including the committed count/revision metadata.
- Dirty state is not evicted by the inactive-session cache.
- The next persistence attempt still uses `save_session_page` with a cursor and revision, rather than treating the partial snapshot as a full save.
- The user sees the persistence error; the app does not retry in a background loop that keeps rewriting snapshots.

An immutable committed baseline also keeps the next run from using volatile or partial state. An older in-flight snapshot may be acknowledged, but the acknowledgement merges into the current live transcript. Its old timestamp and message IDs must not be copied onto a different snapshot.

## Verification

Focused checks run for this change:

```powershell
npx vitest run src/hooks/useAgent.pagination.test.tsx src/components/ChatCanvas.pagination.test.tsx src/components/VirtualizedMessageList.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml history_ --lib --locked -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml materialized_recovery --lib --locked
```

Before final delivery, the full frontend suite, production frontend build, complete Rust suite, and Windows release bundle were also checked. Older verification artifacts remain under `.local/` or the release archive; none were deleted.

### Lazy-history test scenarios

Frontend tests cover:

- Opening a large session through the tail API, including partial context metadata.
- Merging pages by message ID without duplicates.
- Keeping fork, compact, and run startup blocked while a required pre-save is pending.
- Retaining the partial cursor after a failed save.
- Handling late paging responses, including run hydration and provider replay updates.
- Loading canonical full history for export and clone.
- Restoring a session's viewport without automatically fetching older pages again.
- Accepting a page for session A while B is visible without changing B's transcript, queue, status, cursor, or viewport.
- Keeping session A clean without replacing its current live transcript when a long-running hydration save acknowledgement arrives.
- Retaining a dirty partial cursor after a checkpoint conflict and a failed canonical refresh, including after a session switch, until a later explicit page save succeeds.

Native tests cover:

- Tail/page reads use the indexed byte range.
- Corrupt or stale sidecars are rebuilt safely.
- Partial writes require a revision and suffix anchor.
- Legacy/full-save APIs reject a `historyWindow`.
- Message and provider replay content stays byte-for-byte equivalent across a partial rewrite of the prefix/tail boundary.
- Updating an unchanged committed baseline skips index rebuilding, and the index revision matches the canonical message revision.
- After primary JSON persistence fails, the recovery head serves the same message IDs, run status, and provider replay through `load_session` and the tail/page APIs.

### Performance evidence

A real large-session fixture was used in Chromium with CDP metrics and Tauri IPC spies. The benchmark fixture contained 120 turns and about 1.17 MB of tool output.

- Initial open: 60 canonical messages, with roughly 21 DOM message shells. The rest were virtualized.
- Initial open used `load_session_tail`; it made no full-session load call.
- One upward user scroll requested one page. The UI then held 100 canonical messages rather than the entire transcript.
- The visible anchor `a-109` had an offset change of **0 px** in the prepend probe. The browser suite also covers late image and measurement reflows.
- The provider boundary was instrumented before completion: every canonical ID from `u-0` through `a-119` reached the run hydration request. The request did not come from a 60/100-message UI window.
- A controlled status-only update kept the same 60-message array reference. The React Profiler showed no transcript rerender for that status commit, and there was no `agent:markdown-render` mark within the measured interval.
- The exact CDP heap figures, parser telemetry, profiler commits, API call counts, and traces were stored as evidence rather than estimated in prose.

The benchmark is reproducible with `scripts/history-pagination-benchmark.ps1`. It starts an isolated Vite UI and localhost provider, seeds one synthetic disposable session under `%TEMP%` (including configuration, agent home, and workspace), and runs a real Chromium probe against the `AgentRunRequest` and `provider::stream_completion_with_options` boundary. It does not read normal configuration, session lists, provider secrets, or the user's workspace. The benchmark's `AgentRunRequest` contains only fixture fields; review configuration does not expose the normal agent home.

Evidence from the rerun:

- `.local/history-pagination-benchmark-20261109T153616633Z/history-pagination-metrics.json`
- `.local/history-pagination-benchmark-20261109T153616633Z/history-pagination-trace.json`
- `.local/history-pagination-benchmark-20261109T153616633Z/history-pagination-browser-suite.json`
- `.local/history-pagination-benchmark-20261109T153616633Z/history-pagination-browser-suite-trace.json`
- `.local/history-pagination-benchmark-20261109T153616633Z/history-pagination-idle.png`
- `.local/history-pagination-benchmark-20261109T153616633Z/history-pagination-scrolled.png`

Metrics from that run:

- Initial open: 60 model messages, 21 DOM shells, about **91.4 ms** of React commit duration, and a **2,229,504-byte** heap delta.
- One prepend: 100 model messages, 21 DOM shells, about **75.0 ms** of React commit duration, and **0 px** of anchor drift.
- Run hydration: **240 canonical messages** received at the provider boundary; the frontend `agent_run` request remained empty because hydration was native-authoritative.
- Controlled status update: no changed message IDs, unchanged array identity, and **0 ms** of transcript render time.
- Browser suite: **3 tests passed**, with no console errors. It checked that the `run_completed` payload contained no full transcript, that an unchanged baseline acknowledgement used the identical skip path (`baselineWriteCount` **1**), and that canonical history stayed identical after switching A to B and back and adding a follow-up.
- Native corruption probe: invalid primary JSON caused the native read to fail explicitly, not return an apparently successful empty session.
