# Targeted hook follow-up — released

Source frozen after focused verification. No commit, build, full suite, or native gate run by this worker; those remain parent-owned.

## Delivered
- IFR-03: exposed artifact setter now updates the viewed LiveSession selection, including updater-function compatibility. Restored sessions select their first artifact. Clearing selection also updates LiveSession.
- IFR-04: Stop during initial persistence retains `stopRequested=true` while idle, suppresses the pending native launch, and skips aborting a run that never started. A new accepted explicit send resets the live pause intent, so later publications cannot resurrect it.
- ST-08: settings saves are serialized; only the latest revision may roll back, to the last successfully persisted settings rather than a failed optimistic predecessor. Failed saves do not poison the queue. Model refresh merges into current settings after fetch and ignores results for a removed/endpoint-edited provider. Custom-model completion does not change the selection in a newly viewed session.
- Workspace select/clear capture origin LiveSession and navigation before awaits, persist metadata on that origin, and only update current workspace projection while still on that navigation. Dialog selection after navigation is discarded before native workspace mutation. Dialog errors are caught. No late callback writes session B's metadata.
- Artifact-library and deep-search errors set `lastError` and reject, preserving caller Retry/error behavior rather than returning an empty success or logging only.
- Memory read/save/init callbacks accept optional `expectedWorkspace`, captured synchronously with `expectedWorkspace ?? workspaceRef.current`. Explicit empty strings are preserved. Parent owns modal and native validation.
- Metadata integration: current rename persists the session first, then always invokes native `rename_session`. Rename and pin operations join the persistence queue and update only their owned fields on the origin live snapshot, including offscreen authoritative snapshots; returned native messages never replace live messages.

## Files
- `src/hooks/useAgent.ts`
- `src/hooks/useAgentLiveSession.ts`
- `src/hooks/useAgent.reflection.test.tsx`
- This report.

## Verification
Observed targeted RED failures before each corresponding fix: restored artifact selection, prestart Stop pause intent, settings save serialization, stale fetched-model merge, custom-model navigation, workspace dialog/select/clear races, both library/search errors, memory workspace IPC arguments, and current/offscreen rename/pin metadata.

Final executed command:
```
npm test -- src/hooks/useAgent.reflection.test.tsx
Test Files 1 passed (1)
Tests 28 passed (28)
```
All original 15 hook tests remain passing, including completed authoritative snapshots and offscreen runs. `git diff --check` on the owned hook files exited 0; Git emitted only LF/CRLF conversion notices. No full-app hook/Canvas queue integration claim: pause intent is verified at the real-hook boundary; parent owns component/full gates.

## Not addressed / not claimed closed
F02 live-cache eviction, checkpoint cadence, streaming byte budget/hidden-window flush fallback; resetPartial attempt semantics; broad mutation tombstone/revision coordination; native begin/abort handshake. Global native workspace ordering and stale git-status requests are not made transactional by this bounded origin-projection repair. Parent retains these residuals.

## Self-check
Accuracy 4/5: 28 hook regressions pass; native execution and full Canvas integration are outside this worker's verification. Completeness 4/5: bounded assigned repairs delivered, broader residuals explicitly retained. Clarity 4/5: named fixes and limits documented; historical audit remains separate. Actionability 4/5: source released for parent gates, no independent build claim. Conciseness 4/5: scoped report only; regression suite necessarily repeats mock IPC scaffolding. Improvement priority: parent full frontend/type gates and real queue integration, then separately scoped lifecycle/budget residuals. The parent should agree with this bounded assessment.
