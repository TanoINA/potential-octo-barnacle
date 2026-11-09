# Residual backlog follow-up

## Implemented scope

- F02/ST-11 checkpoint slice: live sessions schedule checkpoints after 2 seconds of changes, including offscreen sessions. Timers flush text/reasoning/Council/terminal buffers independently of RAF. One checkpoint per session can be queued/in flight; final saves share the persistence queue. Done cancels timers; generation guards prevent rearming after unmount. Native checkpoint_session uses synced temporary-file + atomic replacement with existing corrupt-file, tombstone, metadata and review protections, without rebuilding FTS every tick.
- RT-06 session boundary: background tasks store sessionId; spawn requires a nonempty owner. Registry list/status/stdin/kill, tools and IPC require the owner session. Delegated tools inherit their origin session. Foreign IDs return not-found. Frontend requests carry the origin session and ignore late navigation results/errors.
- P18 bounded execution slice: native Git commands, including commit/push and a registered fetch IPC, have a 30-second deadline covering exit and output EOF. Cleanup has three separately bounded 1-second waits. Output capture is limited to 1 MiB per stream and overflow fails closed. Git/GCM prompts are disabled without replacing configured SSH transports. Foreground shell tools retain their existing bounded 1..600-second timeout and inherit noninteractive Git configuration. Explicit background daemons retain their intentional long-running lifecycle.
- UI-11: the root cause was in ArtifactPane, not ChatCanvas. Automatic artifact changes preserve user-selected Live Shell and its mounted input/draft; explicit viewer-tab switching still works.
- UI-14: root menu and submenu have max-h-[80vh] overflow-y-auto. Internal scrolling no longer dismisses them. Capture-phase menu keys cannot also submit the composer.

## Verification

- cargo test --manifest-path src-tauri/Cargo.toml: 202 passed, 0 failed, 2 ignored. Both ignored tests are subprocess fixtures invoked explicitly by regressions. Main/doc tests pass.
- npm test -- --fileParallelism=false: 26 test files, 183 passed, 0 failed.
- npx tsc --noEmit: passed.
- git diff --check: passed (LF/CRLF advisories only).

Evidence: verification/residual-rust-tests.log, verification/residual-frontend-tests.log, verification/residual-types.log, verification/residual-whitespace.log.

Regression coverage includes suspended RAF, offscreen checkpoint/final-save ordering, unmount timer rearming, partial disk roundtrip, task ownership/filtering/foreign-ID denial/empty owner, frontend navigation races, Git deadline/output overflow/noninteractive environment, artifact arrival during Live Shell, and menu scrolling/keyboard isolation.

## Remaining boundaries

- Periodic snapshot recovery is not a native event journal: output after the last successful checkpoint can be lost. Timer throttling, disk errors/latency and hardware affect the window; no literal power-cut test was performed. Total live-cache/buffer budget remains outside this slice, so ST-11 is partial.
- Ownership is session-scoped as requested. Parent/child and later runs in the same session share ownership; separate run/subagent capabilities are not implemented. Arbitrary shell execution is not an OS sandbox.
- Git timeout returns are bounded. Windows Job Objects and reliable descendant cleanup after direct-parent exit remain residual; tree termination is best effort. Live remote authentication and packaged WebView were not tested.
- Keyboard-selected context-menu items are not automatically scrolled into view; this is not a complete accessibility redesign.
- AetherAgent.exe was already modified before this work and was not edited/rebuilt. No commit or push was made.
